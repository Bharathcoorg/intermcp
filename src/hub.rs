use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
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
                "clientInfo": { "name": "intermcp-hub", "version": "0.1.0" }
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

        let resp = self.send_request(id, req.to_string()).await?;
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
        cmd.args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        crate::reaper::configure_child_isolation(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| {
            FastMcpError::ToolExecution(format!(
                "Failed to spawn upstream '{}': {}",
                config.name, e
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdout".into())
        })?;
        let reader = BufReader::new(stdout).lines();

        Ok((child, stdin, reader))
    }

    async fn run(mut self) {
        let backoff_delays = [
            Duration::from_millis(100),
            Duration::from_millis(500),
            Duration::from_millis(2000),
        ];

        let mut respawn_attempts = 0;

        loop {
            let (child, mut stdin, mut reader) = match Self::spawn_process(&self.config).await {
                Ok(res) => {
                    respawn_attempts = 0;
                    res
                }
                Err(e) => {
                    error!("Failed to spawn upstream '{}': {}", self.config.name, e);
                    if respawn_attempts < backoff_delays.len() {
                        tokio::time::sleep(backoff_delays[respawn_attempts]).await;
                        respawn_attempts += 1;
                        continue;
                    } else {
                        error!(
                            "Max respawn attempts reached for '{}'. Terminating.",
                            self.config.name
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
                warn!("Upstream '{}' stdout stream closed", name_clone);
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
                    self.config.name, backoff_delays[respawn_attempts]
                );
                tokio::time::sleep(backoff_delays[respawn_attempts]).await;
                respawn_attempts += 1;
            } else {
                error!(
                    "Upstream '{}' exceeded maximum restart attempts",
                    self.config.name
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

#[derive(Debug, Default, Clone)]
pub struct SupplyChainFirewall {
    pinned_contracts: Arc<RwLock<HashMap<String, PinnedToolContract>>>,
    quarantined_upstreams: Arc<RwLock<HashSet<String>>>,
}

impl SupplyChainFirewall {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn compute_hashes(desc: &str, schema: &Value) -> (String, String) {
        let mut desc_hasher = Sha256::new();
        desc_hasher.update(desc.as_bytes());
        let desc_hash = format!("{:x}", desc_hasher.finalize());

        let mut schema_hasher = Sha256::new();
        let schema_bytes = serde_json::to_vec(schema).unwrap_or_default();
        schema_hasher.update(&schema_bytes);
        let schema_hash = format!("{:x}", schema_hasher.finalize());

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
        info!("Spawning upstream server '{}'...", srv_cfg.name);
        match UpstreamHandle::spawn(srv_cfg.clone()).await {
            Ok(handle) => match handle.list_tools().await {
                Ok(tools) => {
                    info!("Discovered {} tools from '{}'", tools.len(), srv_cfg.name);
                    let shared_handle = Arc::new(handle);
                    for tool in tools {
                        let prefixed = format!("{}__{}", srv_cfg.name, tool.name);
                        if registered_names.contains(&prefixed) {
                            warn!("Supply-Chain Collision: Skipping duplicate tool '{}' from upstream '{}'.", prefixed, srv_cfg.name);
                            continue;
                        }

                        if let Some(fw) = &firewall {
                            if let Err(e) = fw.verify_and_pin(&srv_cfg.name, &tool) {
                                error!("Supply-Chain Firewall drift detected: {}", e);
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
                    error!("Failed to list tools from '{}': {}", srv_cfg.name, e);
                }
            },
            Err(e) => {
                error!("Failed to initialize upstream '{}': {}", srv_cfg.name, e);
            }
        }
    }

    Ok(all_tools)
}

pub async fn load_hub_tools(config: HubConfig) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    load_hub_tools_with_firewall(config, Some(SupplyChainFirewall::new())).await
}
