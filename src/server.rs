use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::cache::ToolCache;
use crate::error::FastMcpError;
use crate::guardrails::GuardrailPolicy;
use crate::prompt::Prompt;
use crate::protocol::*;
use crate::resource::Resource;
use crate::tool::Tool;

pub struct Server {
    name: String,
    version: String,
    tools: HashMap<String, Arc<dyn Tool>>,
    resources: HashMap<String, Arc<dyn Resource>>,
    prompts: HashMap<String, Arc<dyn Prompt>>,
    cache: Option<Arc<ToolCache>>,
    guardrail: Option<Arc<GuardrailPolicy>>,
}

impl Server {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            tools: HashMap::new(),
            resources: HashMap::new(),
            prompts: HashMap::new(),
            cache: None,
            guardrail: None,
        }
    }

    pub fn with_cache(mut self, ttl: Duration) -> Self {
        self.cache = Some(Arc::new(ToolCache::new(ttl)));
        self
    }

    pub fn with_guardrails(mut self, max_calls_per_minute: u32, loop_threshold: u32) -> Self {
        self.guardrail = Some(Arc::new(GuardrailPolicy::new(
            max_calls_per_minute,
            loop_threshold,
        )));
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
                debug!("Received notification: method={}", req.method);
                return None;
            }
        };

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

            // --- Tools ---
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
                        // 1. Guardrail Check (Loop breaker & rate limiting)
                        if let Some(guardrail) = &self.guardrail {
                            if let Err(e) = guardrail.check_call(name, &arguments) {
                                let call_err = CallToolResult::error(e.to_string());
                                return Some(JsonRpcResponse::success(req_id, json!(call_err)));
                            }
                        }

                        match self.tools.get(name) {
                            Some(tool) => {
                                // Micro-cache Check (only for deterministic, read-only tools marked cacheable)
                                if tool.is_cacheable() {
                                    if let Some(cache) = &self.cache {
                                        if let Some(cached_val) = cache.get(name, &arguments) {
                                            debug!("Cache HIT for tool: {}", name);
                                            return Some(JsonRpcResponse::success(
                                                req_id, cached_val,
                                            ));
                                        }
                                    }
                                }

                                match tool.execute(arguments.clone()).await {
                                    Ok(tool_result) => {
                                        let json_res = json!(tool_result);
                                        if tool.is_cacheable() {
                                            if let Some(cache) = &self.cache {
                                                cache.set(name, &arguments, json_res.clone(), None);
                                            }
                                        }
                                        Some(JsonRpcResponse::success(req_id, json_res))
                                    }
                                    Err(e) => {
                                        let call_err = CallToolResult::error(e.to_string());
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

            // --- Resources ---
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

            // --- Prompts ---
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

        match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => {
                if let Some(resp) = self.handle_request(req).await {
                    match serde_json::to_string(&resp) {
                        Ok(json_str) => Some(json_str),
                        Err(e) => {
                            error!("Failed to serialize JSON-RPC response: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            }
            Err(e) => {
                error!("Invalid JSON-RPC request: {}", e);
                let err_resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                    None,
                );
                serde_json::to_string(&err_resp).ok()
            }
        }
    }

    pub async fn run_stdio(&self) -> Result<(), FastMcpError> {
        info!(
            "Starting intermcp stdio engine: {} v{} ({} tools, {} resources, {} prompts)",
            self.name,
            self.version,
            self.tools.len(),
            self.resources.len(),
            self.prompts.len()
        );

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin).lines();

        while let Some(line) = reader.next_line().await? {
            if let Some(response_line) = self.handle_raw_message(&line).await {
                stdout.write_all(response_line.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        info!("intermcp stdio stream closed cleanly.");
        Ok(())
    }
}
