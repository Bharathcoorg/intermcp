use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

use crate::error::FastMcpError;
use crate::protocol::{CallToolResult, JsonRpcResponse, ToolDefinition};
use crate::server::redact_for_log;
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

pub const DANGEROUS_ENV_VARS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "PERL5OPT",
    "PERL5LIB",
    "RUBYOPT",
    "RUBYLIB",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "PS4",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub servers: Vec<UpstreamServerConfig>,
}

struct HubRequest {
    id: u64,
    payload: String,
    response_tx: oneshot::Sender<Result<JsonRpcResponse, FastMcpError>>,
}

type PendingMap = Arc<RwLock<HashMap<u64, oneshot::Sender<Result<JsonRpcResponse, FastMcpError>>>>>;

pub struct UpstreamHandle {
    name: String,
    tx: mpsc::Sender<HubRequest>,
    request_counter: AtomicU64,
}

impl UpstreamHandle {
    pub async fn spawn(config: UpstreamServerConfig) -> Result<Self, FastMcpError> {
        if config.name.contains("__") {
            return Err(FastMcpError::SecurityViolation(format!(
                "Upstream server name '{}' cannot contain '__' to prevent namespace collision",
                config.name
            )));
        }

        let (tx, rx) = mpsc::channel(32);
        let name = config.name.clone();

        let supervisor = UpstreamSupervisor::new(config, rx);
        tokio::spawn(async move {
            supervisor.run().await;
        });

        let handle = Self {
            name,
            tx,
            request_counter: AtomicU64::new(1),
        };

        handle.initialize().await?;

        Ok(handle)
    }

    async fn initialize(&self) -> Result<(), FastMcpError> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": { "name": "intermcp-hub", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        self.send_request(1, req.to_string()).await?;
        Ok(())
    }

    async fn send_request(
        &self,
        id: u64,
        payload: String,
    ) -> Result<JsonRpcResponse, FastMcpError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        let hub_req = HubRequest {
            id,
            payload,
            response_tx: resp_tx,
        };

        self.tx.send(hub_req).await.map_err(|_| {
            FastMcpError::ToolExecution(format!("Upstream '{}' supervisor stopped", self.name))
        })?;

        match tokio::time::timeout(Duration::from_secs(30), resp_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(FastMcpError::ToolExecution(format!(
                "Upstream '{}' response channel dropped",
                self.name
            ))),
            Err(_) => Err(FastMcpError::ToolExecution(format!(
                "Upstream '{}' request timed out after 30 seconds",
                self.name
            ))),
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>, FastMcpError> {
        let id = self.request_counter.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {}
        });

        let resp = self.send_request(id, req.to_string()).await?;
        if let Some(res) = resp.result {
            let list_res: crate::protocol::ListToolsResult =
                serde_json::from_value(res).map_err(FastMcpError::Serialization)?;
            Ok(list_res.tools)
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        args: Value,
    ) -> Result<CallToolResult, FastMcpError> {
        let id = self.request_counter.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": args
            }
        });

        let timeout_duration = Duration::from_secs(30);
        let resp =
            match tokio::time::timeout(timeout_duration, self.send_request(id, req.to_string()))
                .await
            {
                Ok(res) => res?,
                Err(_) => {
                    return Err(FastMcpError::ToolExecution(format!(
                        "Upstream tool execution timed out after {:?}",
                        timeout_duration
                    )));
                }
            };

        if let Some(res) = resp.result {
            let tool_res: CallToolResult =
                serde_json::from_value(res).map_err(FastMcpError::Serialization)?;
            Ok(tool_res)
        } else if let Some(err) = resp.error {
            Ok(CallToolResult::error(format!(
                "Upstream error [{}]: {}",
                err.code, err.message
            )))
        } else {
            Ok(CallToolResult::error("Empty upstream response"))
        }
    }
}

struct UpstreamSupervisor {
    config: UpstreamServerConfig,
    rx: mpsc::Receiver<HubRequest>,
}

impl UpstreamSupervisor {
    fn new(config: UpstreamServerConfig, rx: mpsc::Receiver<HubRequest>) -> Self {
        Self { config, rx }
    }

    async fn spawn_process(
        config: &UpstreamServerConfig,
    ) -> Result<
        (
            Child,
            crate::reaper::ChildIsolationGuard,
            ChildStdin,
            tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
        ),
        FastMcpError,
    > {
        let mut cmd = Command::new(&config.command);
        cmd.env_clear();
        for &key in crate::tools::system::SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }
        let is_secret_name = |k: &str| -> bool {
            let k_upper = k.to_uppercase();
            k_upper.contains("SECRET")
                || k_upper.contains("TOKEN")
                || k_upper.contains("PASSWORD")
                || k_upper.contains("KEY")
                || k_upper.contains("AUTH")
                || k_upper.contains("CREDENTIAL")
                || k_upper.contains("CRED")
                || k_upper.contains("PRIVATE")
                || k_upper.contains("CERT")
                || k_upper.contains("PASSPHRASE")
                || k_upper.contains("MNEMONIC")
                || k_upper.contains("SEED_PHRASE")
        };

        let looks_like_secret_value = |v: &str| -> bool {
            let trimmed = v.trim();
            // Check hex string 32+ characters
            if trimmed.len() >= 32 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                return true;
            }
            // Check base64-encoded string 32+ characters
            if trimmed.len() >= 32
                && trimmed
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
            {
                let has_upper = trimmed.chars().any(|c| c.is_ascii_uppercase());
                let has_lower = trimmed.chars().any(|c| c.is_ascii_lowercase());
                let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
                let has_b64_punct =
                    trimmed.contains('+') || trimmed.contains('/') || trimmed.contains('=');
                if (has_upper && has_lower && has_digit) || has_b64_punct {
                    return true;
                }
            }
            false
        };

        let filtered_env: std::collections::HashMap<_, _> = config
            .env
            .iter()
            .filter(|(k, v)| {
                let is_dangerous = DANGEROUS_ENV_VARS
                    .iter()
                    .any(|&d| d.eq_ignore_ascii_case(k));
                !is_dangerous && !is_secret_name(k) && !looks_like_secret_value(v)
            })
            .collect();

        cmd.args(&config.args)
            .envs(filtered_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::reaper::configure_child_isolation(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| {
            FastMcpError::ToolExecution(format!(
                "Failed to spawn upstream '{}': {}",
                config.name, e
            ))
        })?;

        let guard = match crate::reaper::ChildIsolationGuard::new(&child) {
            Ok(g) => g,
            Err(e) => {
                let _ = child.start_kill();
                return Err(e);
            }
        };

        let stdin = child.stdin.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdout".into())
        })?;
        let reader = BufReader::new(stdout).lines();

        if let Some(err_pipe) = child.stderr.take() {
            let upstream_name = config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(err_pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let sanitized = redact_for_log(&line);
                    warn!(target: "intermcp::upstream", upstream = %upstream_name, "{}", sanitized);
                }
            });
        }

        Ok((child, guard, stdin, reader))
    }

    async fn run(mut self) {
        let backoff_delays = [
            Duration::from_millis(100),
            Duration::from_millis(500),
            Duration::from_millis(2000),
        ];

        let mut respawn_attempts = 0;

        loop {
            let (child, guard, mut stdin, mut reader) =
                match Self::spawn_process(&self.config).await {
                    Ok(res) => {
                        respawn_attempts = 0;
                        res
                    }
                    Err(e) => {
                        error!(
                            "Failed to spawn upstream '{}': {}",
                            redact_for_log(&self.config.name),
                            redact_for_log(&e.to_string())
                        );
                        if respawn_attempts < backoff_delays.len() {
                            tokio::time::sleep(backoff_delays[respawn_attempts]).await;
                            respawn_attempts += 1;
                            continue;
                        } else {
                            error!(
                                "Max respawn attempts reached for '{}'. Terminating.",
                                redact_for_log(&self.config.name)
                            );
                            return;
                        }
                    }
                };

            let pending_map: PendingMap = Arc::new(RwLock::new(HashMap::new()));

            let reader_pending = Arc::clone(&pending_map);
            let name_clone = self.config.name.clone();

            let mut reader_task = tokio::spawn(async move {
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                        if let Some(id_num) = resp.id.as_u64() {
                            if let Some(tx) = reader_pending.write().remove(&id_num) {
                                let _ = tx.send(Ok(resp));
                            }
                        } else if let Some(id_str) = resp.id.as_str() {
                            if let Ok(id_parsed) = id_str.parse::<u64>() {
                                if let Some(tx) = reader_pending.write().remove(&id_parsed) {
                                    let _ = tx.send(Ok(resp));
                                }
                            }
                        }
                    }
                }
                warn!(
                    "Upstream '{}' stdout stream closed",
                    redact_for_log(&name_clone)
                );
            });

            let mut process_failed = false;

            while !process_failed {
                tokio::select! {
                    Some(req) = self.rx.recv() => {
                        pending_map.write().insert(req.id, req.response_tx);
                        let write_res = stdin.write_all(req.payload.as_bytes()).await;
                        if write_res.is_ok() {
                            let _ = stdin.write_all(b"\n").await;
                            let flush_res = stdin.flush().await;
                            if flush_res.is_err() {
                                process_failed = true;
                            }
                        } else {
                            process_failed = true;
                        }
                    }
                    _ = &mut reader_task => {
                        process_failed = true;
                    }
                }
            }

            reader_task.abort();
            // AUDIT-06: Explicitly kill process group before dropping child
            // to ensure grandchildren (if any spawned with setsid) are terminated
            guard.kill_group();
            drop(child);

            for (_, tx) in pending_map.write().drain() {
                let _ = tx.send(Err(FastMcpError::ToolExecution(format!(
                    "Upstream '{}' process terminated mid-execution",
                    self.config.name
                ))));
            }

            if respawn_attempts < backoff_delays.len() {
                info!(
                    "Auto-respawning upstream '{}' in {:?}...",
                    redact_for_log(&self.config.name),
                    backoff_delays[respawn_attempts]
                );
                tokio::time::sleep(backoff_delays[respawn_attempts]).await;
                respawn_attempts += 1;
            } else {
                error!(
                    "Upstream '{}' exceeded maximum restart attempts",
                    redact_for_log(&self.config.name)
                );
                break;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedToolContract {
    pub upstream_name: String,
    pub tool_name: String,
    pub description_hash: String,
    pub schema_hash: String,
}

#[derive(Debug, Clone)]
pub struct SupplyChainFirewall {
    pinned_contracts: Arc<RwLock<HashMap<String, PinnedToolContract>>>,
    quarantined_upstreams: Arc<RwLock<HashSet<String>>>,
    quarantine_path: Option<PathBuf>,
}

impl Default for SupplyChainFirewall {
    fn default() -> Self {
        Self::new()
    }
}

impl SupplyChainFirewall {
    pub fn new() -> Self {
        Self {
            pinned_contracts: Arc::new(RwLock::new(HashMap::new())),
            quarantined_upstreams: Arc::new(RwLock::new(HashSet::new())),
            quarantine_path: None,
        }
    }

    pub fn with_quarantine_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        let p = path.into();
        let mut set = self.quarantined_upstreams.read().clone();
        if p.exists() {
            if let Ok(file) = std::fs::File::open(&p) {
                if let Ok(loaded) = serde_json::from_reader::<_, HashSet<String>>(file) {
                    set.extend(loaded);
                }
            }
        }
        self.quarantined_upstreams = Arc::new(RwLock::new(set));
        self.quarantine_path = Some(p);
        self
    }

    pub fn with_receipts_dir<P: Into<PathBuf>>(self, receipts_dir: P) -> Self {
        self.with_quarantine_path(receipts_dir.into().join("quarantine.json"))
    }

    fn persist_quarantine(&self) {
        if let Some(path) = &self.quarantine_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let set = self.quarantined_upstreams.read();
            if let Ok(json_bytes) = serde_json::to_vec_pretty(&*set) {
                let parent_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
                let temp_path = parent_dir.join(format!(
                    ".quarantine.{}.tmp",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                if std::fs::write(&temp_path, &json_bytes).is_ok() {
                    let _ = std::fs::rename(&temp_path, path);
                }
            }
        }
    }

    pub fn compute_hashes(desc: &str, schema: &Value) -> (String, String) {
        let mut desc_hasher = Sha256::new();
        desc_hasher.update(desc.as_bytes());
        let desc_hash = format!("{:x}", desc_hasher.finalize());

        let schema_hash = match crate::receipts::hash_canonical_json(schema) {
            Ok(h) => h,
            Err(_) => {
                let mut schema_hasher = Sha256::new();
                let schema_bytes = serde_json::to_vec(schema).unwrap_or_default();
                schema_hasher.update(&schema_bytes);
                format!("{:x}", schema_hasher.finalize())
            }
        };

        (desc_hash, schema_hash)
    }

    pub fn verify_and_pin(
        &self,
        upstream_name: &str,
        tool: &ToolDefinition,
    ) -> Result<PinnedToolContract, FastMcpError> {
        if self.quarantined_upstreams.read().contains(upstream_name) {
            return Err(FastMcpError::SecurityViolation(format!(
                "Upstream server '{}' is quarantined due to detected supply-chain drift.",
                upstream_name
            )));
        }

        let (desc_hash, schema_hash) = Self::compute_hashes(&tool.description, &tool.input_schema);
        let contract_key = format!("{}__{}", upstream_name, tool.name);

        let mut pinned = self.pinned_contracts.write();
        if let Some(existing) = pinned.get(&contract_key) {
            if existing.description_hash != desc_hash || existing.schema_hash != schema_hash {
                self.quarantined_upstreams
                    .write()
                    .insert(upstream_name.to_string());
                self.persist_quarantine();
                return Err(FastMcpError::SecurityViolation(format!(
                    "Supply-Chain Firewall: Upstream '{}' drifted tool '{}' definition. Quarantining upstream.",
                    upstream_name, tool.name
                )));
            }
            Ok(existing.clone())
        } else {
            let contract = PinnedToolContract {
                upstream_name: upstream_name.to_string(),
                tool_name: tool.name.clone(),
                description_hash: desc_hash,
                schema_hash,
            };
            pinned.insert(contract_key, contract.clone());
            Ok(contract)
        }
    }

    pub fn is_quarantined(&self, upstream_name: &str) -> bool {
        self.quarantined_upstreams.read().contains(upstream_name)
    }

    pub fn quarantine(&self, upstream_name: &str) {
        self.quarantined_upstreams
            .write()
            .insert(upstream_name.to_string());
        self.persist_quarantine();
    }

    pub fn list_contracts(&self) -> Vec<PinnedToolContract> {
        self.pinned_contracts.read().values().cloned().collect()
    }
}

pub struct ProxiedTool {
    #[allow(dead_code)]
    upstream_name: String,
    original_tool_name: String,
    prefixed_name: String,
    description: String,
    input_schema: Value,
    handle: Arc<UpstreamHandle>,
    firewall: Option<SupplyChainFirewall>,
}

#[async_trait::async_trait]
impl Tool for ProxiedTool {
    fn name(&self) -> &str {
        &self.prefixed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, arguments: Value) -> Result<CallToolResult, FastMcpError> {
        if let Some(fw) = &self.firewall {
            if fw.is_quarantined(&self.upstream_name) {
                return Err(FastMcpError::SecurityViolation(format!(
                    "Execution vetoed: Upstream server '{}' is quarantined by Supply-Chain Firewall.",
                    self.upstream_name
                )));
            }
        }
        self.handle
            .call_tool(&self.original_tool_name, arguments)
            .await
    }
}

pub async fn load_hub_tools_with_firewall(
    config: HubConfig,
    firewall: Option<SupplyChainFirewall>,
) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();
    let mut registered_names = HashSet::new();

    for srv_cfg in config.servers {
        if srv_cfg.name.contains("__")
            || srv_cfg.name.starts_with('_')
            || srv_cfg.name.ends_with('_')
        {
            return Err(FastMcpError::SecurityViolation(format!(
                "Upstream server name '{}' cannot contain '__' or start/end with '_' to prevent namespace collision",
                srv_cfg.name
            )));
        }
        info!(
            "Spawning upstream server '{}'...",
            redact_for_log(&srv_cfg.name)
        );
        match UpstreamHandle::spawn(srv_cfg.clone()).await {
            Ok(handle) => match handle.list_tools().await {
                Ok(tools) => {
                    info!(
                        "Discovered {} tools from '{}'",
                        tools.len(),
                        redact_for_log(&srv_cfg.name)
                    );
                    let shared_handle = Arc::new(handle);
                    for tool in tools {
                        if tool.name.contains("__") || tool.name.starts_with('_') {
                            warn!(
                                "Supply-Chain Collision: Skipping tool '{}' with invalid namespace characters from upstream '{}'.",
                                redact_for_log(&tool.name),
                                redact_for_log(&srv_cfg.name)
                            );
                            continue;
                        }
                        let prefixed = format!("{}__{}", srv_cfg.name, tool.name);
                        if registered_names.contains(&prefixed) {
                            warn!("Supply-Chain Collision: Skipping duplicate tool '{}' from upstream '{}'.", redact_for_log(&prefixed), redact_for_log(&srv_cfg.name));
                            continue;
                        }

                        if let Some(fw) = &firewall {
                            if let Err(e) = fw.verify_and_pin(&srv_cfg.name, &tool) {
                                error!(
                                    "Supply-Chain Firewall drift detected: {}",
                                    redact_for_log(&e.to_string())
                                );
                                return Err(e);
                            }
                        }

                        registered_names.insert(prefixed.clone());
                        all_tools.push(Box::new(ProxiedTool {
                            upstream_name: srv_cfg.name.clone(),
                            original_tool_name: tool.name,
                            prefixed_name: prefixed,
                            description: tool.description,
                            input_schema: tool.input_schema,
                            handle: Arc::clone(&shared_handle),
                            firewall: firewall.clone(),
                        }));
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to list tools from '{}': {}",
                        redact_for_log(&srv_cfg.name),
                        redact_for_log(&e.to_string())
                    );
                }
            },
            Err(e) => {
                error!(
                    "Failed to initialize upstream '{}': {}",
                    redact_for_log(&srv_cfg.name),
                    redact_for_log(&e.to_string())
                );
            }
        }
    }

    Ok(all_tools)
}

pub async fn load_hub_tools(config: HubConfig) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    load_hub_tools_with_firewall(
        config,
        Some(SupplyChainFirewall::new().with_receipts_dir("receipts")),
    )
    .await
}
