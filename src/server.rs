use aho_corasick::AhoCorasick;
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::cache::ToolCache;
use crate::error::FastMcpError;
use crate::guardrails::GuardrailPolicy;
use crate::policy::{PolicyEngine, ShellPolicyDecision};
use crate::prompt::Prompt;
use crate::protocol::*;
use crate::receipts::{ReceiptBook, ReceiptStatus};
use crate::record::{FrameDirection, SessionRecorder};
use crate::resource::Resource;
use crate::smac::SmacLogger;
use crate::taint::{SinkCapability, TaintTracker};
use crate::tool::Tool;
use crate::vault_lock::TimeLockedVault;

static GLOBAL_LOG_LEVEL: AtomicUsize = AtomicUsize::new(2);

pub const DEFAULT_SECRET_ENV_NAMES: &[&str] = &[
    "API_KEY",
    "SECRET",
    "TOKEN",
    "PASSWORD",
    "PRIVATE_KEY",
    "JWT",
    "BEARER",
    "COOKIE",
    "SESSION",
    "DSN",
    "CONNECTION_STRING",
    "MNEMONIC",
    "SEED",
    "WALLET",
    "OAUTH",
    "REFRESH",
    "CLIENT_SECRET",
    "SIGNING",
    "TEST_JWT_SECRET",
    "TEST_BEARER_AUTH",
    "TEST_CLIENT_SECRET_VAL",
    "TEST_MNEMONIC_KEY",
    "TEST_DATABASE_DSN_SECRET",
];

pub fn mask_secrets(text: &str) -> String {
    mask_secrets_with_names(text, DEFAULT_SECRET_ENV_NAMES)
}

pub fn mask_secrets_with_names<S: AsRef<str>>(text: &str, names: &[S]) -> String {
    if names.is_empty() {
        return text.to_string();
    }
    let mut patterns = Vec::new();
    for (k, v) in std::env::vars() {
        let k_upper = k.to_uppercase();
        let is_allowed_name = names.iter().any(|n| {
            let n_upper = n.as_ref().to_uppercase();
            k_upper == n_upper || k_upper.contains(&n_upper)
        });
        if is_allowed_name && v.len() >= 8 {
            patterns.push(v);
        }
    }
    if patterns.is_empty() {
        return text.to_string();
    }

    patterns.sort_by_key(|p| std::cmp::Reverse(p.len()));

    if let Ok(matcher) = AhoCorasick::builder().build(&patterns) {
        let replacements = vec!["[REDACTED_BY_INTERMCP]"; patterns.len()];
        matcher.replace_all(text, &replacements)
    } else {
        text.to_string()
    }
}

pub fn redact_for_log(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }

    let mut matches: Vec<(usize, usize)> = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();

    // 1. Authorization:\s*Bearer\s+[^\s]+
    for (i, _) in s.char_indices() {
        let remaining = &s[i..];
        let lower = remaining.to_ascii_lowercase();
        if lower.starts_with("authorization:") {
            let start = i;
            let mut j = i + 14;
            while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < len && s.is_char_boundary(j) && s[j..].to_ascii_lowercase().starts_with("bearer")
            {
                j += 6;
                if j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                        j += 1;
                    }
                    let val_start = j;
                    while j < len && !bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j > val_start {
                        matches.push((start, j));
                    }
                }
            }
        }
    }

    // 2. Bearer [A-Za-z0-9_\-\.=]{8,}
    for (i, _) in s.char_indices() {
        if s[i..].starts_with("Bearer ") {
            let start = i;
            let mut j = i + 7;
            while j < len
                && (bytes[j].is_ascii_alphanumeric()
                    || bytes[j] == b'_'
                    || bytes[j] == b'-'
                    || bytes[j] == b'.'
                    || bytes[j] == b'=')
            {
                j += 1;
            }
            if j - (i + 7) >= 8 {
                matches.push((start, j));
            }
        }
    }

    // 3. (?i)(api[_-]?key|secret|token|password|signing[_-]?key)["'\s:=]+[A-Za-z0-9_\-\.=]{12,}
    const KEYWORDS: &[&str] = &[
        "api_key",
        "api-key",
        "apikey",
        "signing_key",
        "signing-key",
        "signingkey",
        "password",
        "secret",
        "token",
    ];

    for (i, _) in s.char_indices() {
        let remaining = &s[i..];
        let lower = remaining.to_ascii_lowercase();
        let mut kw_len = 0;
        for kw in KEYWORDS {
            if lower.starts_with(kw) {
                kw_len = kw.len();
                break;
            }
        }

        if kw_len > 0 {
            let start = i;
            let mut j = i + kw_len;
            let sep_start = j;
            while j < len
                && (bytes[j] == b'"'
                    || bytes[j] == b'\''
                    || bytes[j] == b' '
                    || bytes[j] == b'\t'
                    || bytes[j] == b'\r'
                    || bytes[j] == b'\n'
                    || bytes[j] == b':'
                    || bytes[j] == b'=')
            {
                j += 1;
            }

            if j > sep_start {
                let val_start = j;
                while j < len
                    && (bytes[j].is_ascii_alphanumeric()
                        || bytes[j] == b'_'
                        || bytes[j] == b'-'
                        || bytes[j] == b'.'
                        || bytes[j] == b'=')
                {
                    j += 1;
                }
                if j - val_start >= 12 {
                    matches.push((start, j));
                }
            }
        }
    }

    if matches.is_empty() {
        return s.to_string();
    }

    matches.sort_by_key(|m| (m.0, m.1));
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in matches {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                if end > last.1 {
                    last.1 = end;
                }
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut result = String::with_capacity(s.len());
    let mut last_idx = 0;
    for (start, end) in merged {
        result.push_str(&s[last_idx..start]);
        result.push_str("[REDACTED]");
        last_idx = end;
    }
    if last_idx < s.len() {
        result.push_str(&s[last_idx..]);
    }
    result
}

pub struct Server {
    name: String,
    version: String,
    tools: HashMap<String, Arc<dyn Tool>>,
    resources: HashMap<String, Arc<dyn Resource>>,
    prompts: HashMap<String, Arc<dyn Prompt>>,
    cache: Option<Arc<ToolCache>>,
    guardrail: Option<Arc<GuardrailPolicy>>,
    cancellations: Arc<RwLock<HashMap<Value, CancellationToken>>>,
    recorder: Option<SessionRecorder>,
    smac: Option<Arc<SmacLogger>>,
    vault_lock: Option<Arc<TimeLockedVault>>,
    receipt_book: Option<Arc<ReceiptBook>>,
    secret_env_names: Vec<String>,
    taint_tracker: Option<Arc<TaintTracker>>,
    policy_engine: Option<Arc<PolicyEngine>>,
    session_id: String,
}

impl Server {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        // Generate unique session ID for receipt tracking
        let session_id = {
            use std::fmt::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let pid = std::process::id();
            let mut s = String::with_capacity(24);
            let _ = write!(s, "{:x}-{:x}", now.as_millis() as u64, pid);
            s
        };

        Self {
            name: name.into(),
            version: version.into(),
            tools: HashMap::new(),
            resources: HashMap::new(),
            prompts: HashMap::new(),
            cache: None,
            guardrail: None,
            cancellations: Arc::new(RwLock::new(HashMap::new())),
            recorder: None,
            smac: None,
            vault_lock: None,
            receipt_book: None,
            secret_env_names: DEFAULT_SECRET_ENV_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            taint_tracker: None,
            policy_engine: None,
            session_id,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn with_policy_engine(mut self, engine: PolicyEngine) -> Self {
        self.policy_engine = Some(Arc::new(engine));
        self
    }

    pub fn policy_engine(&self) -> Option<Arc<PolicyEngine>> {
        self.policy_engine.clone()
    }

    pub fn with_taint_tracker(mut self, tracker: TaintTracker) -> Self {
        self.taint_tracker = Some(Arc::new(tracker));
        self
    }

    pub fn taint_tracker(&self) -> Option<Arc<TaintTracker>> {
        self.taint_tracker.clone()
    }

    pub fn with_secret_env_names(mut self, names: Vec<String>) -> Self {
        self.secret_env_names = names;
        self
    }

    pub fn add_secret_env_name(&mut self, name: impl Into<String>) {
        self.secret_env_names.push(name.into());
    }

    pub fn redact_tool_result(&self, result: &mut CallToolResult) {
        if self.secret_env_names.is_empty() {
            return;
        }
        for item in &mut result.content {
            match item {
                ContentItem::Text { text } => {
                    *text = mask_secrets_with_names(text, &self.secret_env_names);
                }
                ContentItem::Image { data, mime_type } => {
                    *data = mask_secrets_with_names(data, &self.secret_env_names);
                    *mime_type = mask_secrets_with_names(mime_type, &self.secret_env_names);
                }
                ContentItem::Resource { resource } => {
                    resource.text = mask_secrets_with_names(&resource.text, &self.secret_env_names);
                    resource.uri = mask_secrets_with_names(&resource.uri, &self.secret_env_names);
                }
            }
        }
    }

    pub fn with_recorder(mut self, recorder: SessionRecorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    pub fn with_smac(mut self, smac: SmacLogger) -> Self {
        self.smac = Some(Arc::new(smac));
        self
    }

    pub fn with_receipt_book(mut self, receipt_book: ReceiptBook) -> Self {
        self.receipt_book = Some(Arc::new(receipt_book));
        self
    }

    pub fn with_time_locked_vault(mut self, vault: TimeLockedVault) -> Self {
        self.vault_lock = Some(Arc::new(vault));
        self
    }

    pub fn vault_lock(&self) -> Option<Arc<TimeLockedVault>> {
        self.vault_lock.clone()
    }

    pub fn with_cache(mut self, ttl: Duration) -> Self {
        self.cache = Some(Arc::new(ToolCache::new(ttl)));
        self
    }

    pub fn with_cache_bytes(mut self, ttl: Duration, max_bytes: usize) -> Self {
        self.cache = Some(Arc::new(ToolCache::with_max_bytes(ttl, max_bytes)));
        self
    }

    pub fn with_guardrails(mut self, max_calls_per_minute: u32, loop_threshold: u32) -> Self {
        self.guardrail = Some(Arc::new(GuardrailPolicy::new(
            max_calls_per_minute,
            loop_threshold,
        )));
        self
    }

    pub fn with_guardrail_policy(mut self, policy: GuardrailPolicy) -> Self {
        self.guardrail = Some(Arc::new(policy));
        self
    }

    pub fn with_token_budget(mut self, estimated_tokens: usize) -> Self {
        self.guardrail = Some(Arc::new(
            GuardrailPolicy::default_policy().with_token_budget(estimated_tokens),
        ));
        self
    }

    pub fn add_tool(&mut self, tool: Box<dyn Tool>) -> &mut Self {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::from(tool));
        self
    }

    pub fn add_tools(&mut self, tools: Vec<Box<dyn Tool>>) -> &mut Self {
        for tool in tools {
            self.add_tool(tool);
        }
        self
    }

    pub fn add_resource(&mut self, resource: Box<dyn Resource>) -> &mut Self {
        let uri = resource.uri().to_string();
        self.resources.insert(uri, Arc::from(resource));
        self
    }

    pub fn add_prompt(&mut self, prompt: Box<dyn Prompt>) -> &mut Self {
        let name = prompt.name().to_string();
        self.prompts.insert(name, Arc::from(prompt));
        self
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    pub fn prompt_count(&self) -> usize {
        self.prompts.len()
    }

    pub fn cache_stats(&self) -> Option<(u64, u64, usize)> {
        self.cache.as_ref().map(|c| c.stats())
    }

    pub fn list_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let req_id = match req.id {
            Some(id) => id,
            None => {
                match req.method.as_str() {
                    "notifications/initialized" => {
                        debug!("Client initialized notification acknowledged");
                    }
                    "notifications/cancelled" => {
                        if let Some(params) = req.params {
                            if let Some(target_id) = params.get("requestId") {
                                if let Some(token) = self.cancellations.read().get(target_id) {
                                    token.cancel();
                                }
                            }
                        }
                    }
                    "shutdown" => {
                        if let Some(guardrail) = &self.guardrail {
                            guardrail.reset();
                        }
                    }
                    _ => {
                        debug!(
                            "Received notification: method={}",
                            redact_for_log(&req.method)
                        );
                    }
                }
                return None;
            }
        };

        if req.jsonrpc != "2.0" {
            return Some(JsonRpcResponse::error(
                req_id,
                -32600,
                "Invalid Request: jsonrpc must be '2.0'".into(),
                None,
            ));
        }

        match req.method.as_str() {
            "initialize" => {
                let result = InitializeResult {
                    protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(CapabilityInfo {
                            list_changed: Some(false),
                        }),
                        resources: Some(CapabilityInfo {
                            list_changed: Some(false),
                        }),
                        prompts: Some(CapabilityInfo {
                            list_changed: Some(false),
                        }),
                    },
                    server_info: Implementation {
                        name: self.name.clone(),
                        version: self.version.clone(),
                    },
                };
                Some(JsonRpcResponse::success(req_id, json!(result)))
            }
            "ping" => Some(JsonRpcResponse::success(req_id, json!({}))),

            "logging/setLevel" => {
                let level_str = req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("level"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("info");

                let numeric_level = match level_str.to_lowercase().as_str() {
                    "error" => 0,
                    "warn" => 1,
                    "info" => 2,
                    "debug" => 3,
                    "trace" => 4,
                    _ => 2,
                };
                GLOBAL_LOG_LEVEL.store(numeric_level, Ordering::Relaxed);
                Some(JsonRpcResponse::success(req_id, json!({})))
            }

            "completion/complete" => {
                let params = req.params.unwrap_or(Value::Null);
                let query = params
                    .get("argument")
                    .and_then(|a| a.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mut completions = Vec::new();
                for (name, tool) in &self.tools {
                    if name.contains(query) || tool.description().contains(query) {
                        completions.push(name.clone());
                    }
                }
                for uri in self.resources.keys() {
                    if uri.contains(query) {
                        completions.push(uri.clone());
                    }
                }
                for name in self.prompts.keys() {
                    if name.contains(query) {
                        completions.push(name.clone());
                    }
                }

                let total = completions.len();
                Some(JsonRpcResponse::success(
                    req_id,
                    json!({
                        "completion": {
                            "values": completions,
                            "total": total,
                            "hasMore": false
                        }
                    }),
                ))
            }

            "tools/list" => {
                let definitions: Vec<ToolDefinition> =
                    self.tools.values().map(|tool| tool.definition()).collect();

                let result = ListToolsResult { tools: definitions };
                Some(JsonRpcResponse::success(req_id, json!(result)))
            }
            "tools/call" => {
                let params = req.params.unwrap_or(Value::Null);
                let tool_name = params.get("name").and_then(|v| v.as_str());
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match tool_name {
                    Some(name) => {
                        if let Some(engine) = &self.policy_engine {
                            if let Err(e) = engine.check_rate_limit(name) {
                                let call_err = CallToolResult::error(e.to_string());
                                return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                            }
                            if name == "system_run_command" {
                                let cmd = arguments
                                    .get("command")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let binary = cmd.split_whitespace().next().unwrap_or("");
                                match engine.check_shell(binary, cmd) {
                                    Ok(ShellPolicyDecision::Allow) => {}
                                    Ok(ShellPolicyDecision::RequireSupervisorApproval(pattern)) => {
                                        let call_err = CallToolResult::error(format!(
                                            "Policy requires supervisor approval for pattern: {}",
                                            pattern
                                        ));
                                        return Some(JsonRpcResponse::success(
                                            req_id,
                                            json!(call_err),
                                        ));
                                    }
                                    Err(e) => {
                                        let call_err = CallToolResult::error(e.to_string());
                                        return Some(JsonRpcResponse::success(
                                            req_id,
                                            json!(call_err),
                                        ));
                                    }
                                }
                            }
                            if name == "fs_read_file"
                                || name == "fs_write_file"
                                || name == "fs_list_dir"
                                || name == "fs_list_directory"
                            {
                                let path_str =
                                    arguments.get("path").and_then(|v| v.as_str()).unwrap_or(
                                        if name == "fs_list_dir" || name == "fs_list_directory" {
                                            "."
                                        } else {
                                            ""
                                        },
                                    );
                                if !path_str.is_empty() {
                                    let is_write = name == "fs_write_file";
                                    if let Err(e) = engine
                                        .check_filesystem(std::path::Path::new(path_str), is_write)
                                    {
                                        let call_err = CallToolResult::error(e.to_string());
                                        return Some(JsonRpcResponse::success(
                                            req_id,
                                            json!(call_err),
                                        ));
                                    }
                                }
                            }
                        }

                        if let Some(guardrail) = &self.guardrail {
                            if let Err(e) = guardrail.check_call(name, &arguments) {
                                let call_err = CallToolResult::error(e.to_string());
                                return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                            }
                        }

                        if let Some(tracker) = &self.taint_tracker {
                            if name == "system_run_command" || name == "fs_write_file" {
                                if let Err(e) = tracker.scan_json_arguments(
                                    &arguments,
                                    SinkCapability::PrivilegedExecution,
                                ) {
                                    let call_err = CallToolResult::error(e.to_string());
                                    return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                                }
                            }
                        }

                        if let Some(vault) = &self.vault_lock {
                            match vault.check_or_wait(name, &arguments).await {
                                Ok(true) => {}
                                Ok(false) => {
                                    let call_err = CallToolResult::error(format!(
                                        "Time-Locked Vault: Execution of '{}' was vetoed or timed out waiting for supervisor approval.",
                                        name
                                    ));
                                    return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                                }
                                Err(e) => {
                                    let call_err = CallToolResult::error(e.to_string());
                                    return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                                }
                            }
                        }

                        match self.tools.get(name) {
                            Some(tool) => {
                                if tool.is_cacheable() {
                                    if let Some(cache) = &self.cache {
                                        if let Some(cached_val) = cache.get(name, &arguments) {
                                            debug!("Cache HIT for tool: {}", redact_for_log(name));
                                            return Some(JsonRpcResponse::success(
                                                req_id, cached_val,
                                            ));
                                        }
                                    }
                                }

                                let cancel_token = CancellationToken::new();
                                let cancel_key = req_id.clone();
                                self.cancellations
                                    .write()
                                    .insert(cancel_key.clone(), cancel_token.clone());

                                let tool_clone = Arc::clone(tool);
                                let args_clone = arguments.clone();
                                let start_instant = std::time::Instant::now();
                                let task_cancel = cancel_token.clone();

                                let mut task = tokio::spawn(async move {
                                    tokio::select! {
                                        _ = task_cancel.cancelled() => Err(FastMcpError::ToolExecution("Cancelled".into())),
                                        res = tool_clone.execute(args_clone) => res,
                                    }
                                });

                                let execution_result = tokio::select! {
                                    _ = cancel_token.cancelled() => {
                                        task.abort();
                                        self.cancellations.write().remove(&cancel_key);
                                        return Some(JsonRpcResponse::error(
                                            req_id,
                                            -32000,
                                            "Request cancelled by client".into(),
                                            None,
                                        ));
                                    }
                                    res = &mut task => {
                                        self.cancellations.write().remove(&cancel_key);
                                        match res {
                                            Ok(call_res) => call_res,
                                            Err(join_err) => {
                                                if join_err.is_panic() {
                                                    return Some(JsonRpcResponse::error(
                                                        req_id,
                                                        -32603,
                                                        "internal tool panic".into(),
                                                        None,
                                                    ));
                                                } else {
                                                    return Some(JsonRpcResponse::error(
                                                        req_id,
                                                        -32000,
                                                        format!("Tool task failed: {}", join_err),
                                                        None,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                };

                                match execution_result {
                                    Ok(mut tool_result) => {
                                        if let Some(engine) = &self.policy_engine {
                                            let mut total_output_bytes = 0;
                                            for item in &tool_result.content {
                                                match item {
                                                    ContentItem::Text { text } => {
                                                        total_output_bytes += text.len()
                                                    }
                                                    ContentItem::Image { data, .. } => {
                                                        total_output_bytes += data.len()
                                                    }
                                                    ContentItem::Resource { resource } => {
                                                        total_output_bytes += resource.text.len()
                                                    }
                                                }
                                            }
                                            if let Err(e) =
                                                engine.check_output_size(total_output_bytes)
                                            {
                                                let call_err = CallToolResult::error(e.to_string());
                                                return Some(JsonRpcResponse::success(
                                                    req_id,
                                                    json!(call_err),
                                                ));
                                            }
                                        }

                                        self.redact_tool_result(&mut tool_result);

                                        if let Some(guardrail) = &self.guardrail {
                                            for item in &tool_result.content {
                                                if let ContentItem::Text { text } = item {
                                                    if let Err(e) = guardrail.record_output(text) {
                                                        let call_err =
                                                            CallToolResult::error(e.to_string());
                                                        return Some(JsonRpcResponse::success(
                                                            req_id,
                                                            json!(call_err),
                                                        ));
                                                    }
                                                }
                                            }
                                        }

                                        let json_res = json!(tool_result);
                                        if let Some(smac) = &self.smac {
                                            smac.record(name, &arguments, &json_res);
                                        }
                                        if let Some(receipt_book) = &self.receipt_book {
                                            let schema_hash = crate::receipts::hash_canonical_json(
                                                &tool.input_schema(),
                                            )
                                            .unwrap_or_default();
                                            let _ = receipt_book.record_execution(
                                                &self.session_id,
                                                name,
                                                &schema_hash,
                                                &arguments,
                                                &json_res,
                                                start_instant.elapsed().as_micros() as u64,
                                                ReceiptStatus::Success,
                                            );
                                        }
                                        if tool.is_cacheable() {
                                            if let Some(cache) = &self.cache {
                                                cache.set(name, &arguments, json_res.clone(), None);
                                            }
                                        }
                                        Some(JsonRpcResponse::success(req_id, json_res))
                                    }
                                    Err(e) => {
                                        let call_err = CallToolResult::error(e.to_string());
                                        if let Some(receipt_book) = &self.receipt_book {
                                            let schema_hash = crate::receipts::hash_canonical_json(
                                                &tool.input_schema(),
                                            )
                                            .unwrap_or_default();
                                            let _ = receipt_book.record_execution(
                                                &self.session_id,
                                                name,
                                                &schema_hash,
                                                &arguments,
                                                &json!(call_err),
                                                start_instant.elapsed().as_micros() as u64,
                                                ReceiptStatus::ToolError,
                                            );
                                        }
                                        Some(JsonRpcResponse::success(req_id, json!(call_err)))
                                    }
                                }
                            }
                            None => {
                                let err_resp = JsonRpcResponse::error(
                                    req_id,
                                    -32602,
                                    format!("Tool not found: {}", name),
                                    None,
                                );
                                Some(err_resp)
                            }
                        }
                    }
                    None => {
                        let err_resp = JsonRpcResponse::error(
                            req_id,
                            -32602,
                            "Missing 'name' in tools/call parameters".to_string(),
                            None,
                        );
                        Some(err_resp)
                    }
                }
            }

            "resources/list" => {
                let definitions: Vec<ResourceDefinition> = self
                    .resources
                    .values()
                    .map(|res| res.definition())
                    .collect();

                let result = ListResourcesResult {
                    resources: definitions,
                };
                Some(JsonRpcResponse::success(req_id, json!(result)))
            }
            "resources/read" => {
                let params = req.params.unwrap_or(Value::Null);
                let uri = params.get("uri").and_then(|v| v.as_str());

                match uri {
                    Some(u) => match self.resources.get(u) {
                        Some(resource) => match resource.read().await {
                            Ok(read_result) => {
                                Some(JsonRpcResponse::success(req_id, json!(read_result)))
                            }
                            Err(e) => Some(JsonRpcResponse::error(
                                req_id,
                                -32000,
                                format!("Resource read failed: {}", e),
                                None,
                            )),
                        },
                        None => Some(JsonRpcResponse::error(
                            req_id,
                            -32602,
                            format!("Resource not found: {}", u),
                            None,
                        )),
                    },
                    None => Some(JsonRpcResponse::error(
                        req_id,
                        -32602,
                        "Missing 'uri' in resources/read parameters".to_string(),
                        None,
                    )),
                }
            }

            "prompts/list" => {
                let definitions: Vec<PromptDefinition> =
                    self.prompts.values().map(|p| p.definition()).collect();

                let result = ListPromptsResult {
                    prompts: definitions,
                };
                Some(JsonRpcResponse::success(req_id, json!(result)))
            }
            "prompts/get" => {
                let params = req.params.unwrap_or(Value::Null);
                let prompt_name = params.get("name").and_then(|v| v.as_str());
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match prompt_name {
                    Some(name) => match self.prompts.get(name) {
                        Some(prompt) => match prompt.get(arguments).await {
                            Ok(get_result) => {
                                Some(JsonRpcResponse::success(req_id, json!(get_result)))
                            }
                            Err(e) => Some(JsonRpcResponse::error(
                                req_id,
                                -32000,
                                format!("Prompt retrieval failed: {}", e),
                                None,
                            )),
                        },
                        None => Some(JsonRpcResponse::error(
                            req_id,
                            -32602,
                            format!("Prompt not found: {}", name),
                            None,
                        )),
                    },
                    None => Some(JsonRpcResponse::error(
                        req_id,
                        -32602,
                        "Missing 'name' in prompts/get parameters".to_string(),
                        None,
                    )),
                }
            }

            other => {
                let err_resp = JsonRpcResponse::error(
                    req_id,
                    -32601,
                    format!("Method not found: {}", other),
                    None,
                );
                Some(err_resp)
            }
        }
    }

    pub async fn handle_raw_message(&self, raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(rec) = &self.recorder {
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                rec.record(FrameDirection::Inbound, &val);
            }
        }

        let resp_opt = self.process_raw_message(trimmed).await;

        if let Some(ref resp_str) = resp_opt {
            if let Some(rec) = &self.recorder {
                if let Ok(val) = serde_json::from_str::<Value>(resp_str) {
                    rec.record(FrameDirection::Outbound, &val);
                }
            }
        }

        resp_opt
    }

    async fn process_raw_message(&self, trimmed: &str) -> Option<String> {
        if trimmed.starts_with('[') {
            let parsed: Result<Vec<Value>, _> = serde_json::from_str(trimmed);
            match parsed {
                Ok(items) => {
                    if items.is_empty() {
                        let err = JsonRpcResponse::error(
                            Value::Null,
                            -32600,
                            "Invalid Request: empty batch".into(),
                            None,
                        );
                        return serde_json::to_string(&err).ok();
                    }
                    if items.len() > 100 {
                        let err = JsonRpcResponse::error(
                            Value::Null,
                            -32600,
                            "Invalid Request: batch too large".into(),
                            None,
                        );
                        return serde_json::to_string(&err).ok();
                    }

                    let mut responses = Vec::new();
                    for item in items {
                        let id = item.get("id").cloned().unwrap_or(Value::Null);
                        let jsonrpc = item.get("jsonrpc").and_then(|v| v.as_str());
                        if jsonrpc != Some("2.0") {
                            responses.push(JsonRpcResponse::error(
                                id,
                                -32600,
                                "Invalid Request: jsonrpc must be '2.0'".into(),
                                None,
                            ));
                            continue;
                        }

                        match serde_json::from_value::<JsonRpcRequest>(item) {
                            Ok(req) => {
                                if let Some(resp) = self.handle_request(req).await {
                                    responses.push(resp);
                                }
                            }
                            Err(e) => {
                                responses.push(JsonRpcResponse::error(
                                    id,
                                    -32600,
                                    format!("Invalid Request: {}", e),
                                    None,
                                ));
                            }
                        }
                    }

                    if responses.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&responses).ok()
                    }
                }
                Err(e) => {
                    let err = JsonRpcResponse::error(
                        Value::Null,
                        -32700,
                        format!("Parse error: {}", e),
                        None,
                    );
                    serde_json::to_string(&err).ok()
                }
            }
        } else {
            let parsed: Result<Value, _> = serde_json::from_str(trimmed);
            match parsed {
                Ok(val) => {
                    let id = val.get("id").cloned().unwrap_or(Value::Null);
                    let jsonrpc = val.get("jsonrpc").and_then(|v| v.as_str());
                    if jsonrpc != Some("2.0") {
                        let err = JsonRpcResponse::error(
                            id,
                            -32600,
                            "Invalid Request: jsonrpc must be '2.0'".into(),
                            None,
                        );
                        return serde_json::to_string(&err).ok();
                    }

                    match serde_json::from_value::<JsonRpcRequest>(val) {
                        Ok(req) => {
                            if let Some(resp) = self.handle_request(req).await {
                                serde_json::to_string(&resp).ok()
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            let err = JsonRpcResponse::error(
                                id,
                                -32600,
                                format!("Invalid Request: {}", e),
                                None,
                            );
                            serde_json::to_string(&err).ok()
                        }
                    }
                }
                Err(e) => {
                    let err = JsonRpcResponse::error(
                        Value::Null,
                        -32700,
                        format!("Parse error: {}", e),
                        None,
                    );
                    serde_json::to_string(&err).ok()
                }
            }
        }
    }

    pub async fn run_stdio(&self) -> Result<(), FastMcpError> {
        info!(
            "InterMCP server '{}' running on stdio",
            redact_for_log(&self.name)
        );

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| FastMcpError::Internal(e.to_string()))?
        {
            if let Some(response_str) = self.handle_raw_message(&line).await {
                stdout
                    .write_all(response_str.as_bytes())
                    .await
                    .map_err(|e| FastMcpError::Internal(e.to_string()))?;
                stdout
                    .write_all(b"\n")
                    .await
                    .map_err(|e| FastMcpError::Internal(e.to_string()))?;
                stdout
                    .flush()
                    .await
                    .map_err(|e| FastMcpError::Internal(e.to_string()))?;
            }
        }

        Ok(())
    }
}
