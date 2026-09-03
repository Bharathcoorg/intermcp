use crate::error::FastMcpError;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Autonomous AI Agent Loop Breaker & Cost Guardrail
/// Prevents runaway autonomous agents from executing infinite loops or draining API budgets.
pub struct GuardrailPolicy {
    max_calls_per_minute: u32,
    loop_detection_threshold: u32,
    max_session_chars: Option<usize>,
    cumulative_chars: RwLock<usize>,
    history: RwLock<HashMap<String, Vec<Instant>>>,
    last_signature: RwLock<Option<(String, u32)>>,
}

impl GuardrailPolicy {
    pub fn new(max_calls_per_minute: u32, loop_detection_threshold: u32) -> Self {
        Self {
            max_calls_per_minute,
            loop_detection_threshold,
            max_session_chars: None,
            cumulative_chars: RwLock::new(0),
            history: RwLock::new(HashMap::new()),
            last_signature: RwLock::new(None),
        }
    }

    pub fn default_policy() -> Self {
        Self::new(60, 5) // 60 calls/min, max 5 consecutive identical calls
    }

    pub fn with_token_budget(mut self, estimated_tokens: usize) -> Self {
        // Approximate 4 chars per token
        self.max_session_chars = Some(estimated_tokens * 4);
        self
    }

    pub fn record_output(&self, text: &str) -> Result<(), FastMcpError> {
        if let Some(limit) = self.max_session_chars {
            if let Ok(mut total) = self.cumulative_chars.write() {
                *total += text.len();
                if *total > limit {
                    return Err(FastMcpError::ToolExecution(format!(
                        "🚨 InterMCP Budget Sentinel: Session token limit reached (exceeded ~{} estimated tokens). Execution paused to prevent runaway API spend.",
                        *total / 4
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn estimated_tokens_used(&self) -> usize {
        self.cumulative_chars.read().map(|t| *t / 4).unwrap_or(0)
    }

    pub fn check_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), FastMcpError> {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        // 1. Velocity rate limiting
        if let Ok(mut hist) = self.history.write() {
            let timestamps = hist.entry(tool_name.to_string()).or_insert_with(Vec::new);
            timestamps.retain(|&t| t > one_minute_ago);

            if timestamps.len() >= self.max_calls_per_minute as usize {
                return Err(FastMcpError::ToolExecution(format!(
                    "🚨 InterMCP Guardrail Triggered: Tool '{}' exceeded rate limit of {} calls/min. Execution paused to protect API budget.",
                    tool_name, self.max_calls_per_minute
                )));
            }

            timestamps.push(now);
        }

        // 2. Infinite consecutive loop detection
        let signature = format!("{}:{}", tool_name, arguments);
        if let Ok(mut last) = self.last_signature.write() {
            let count = match &*last {
                Some((prev_sig, c)) if prev_sig == &signature => c + 1,
                _ => 1,
            };

            if count > self.loop_detection_threshold {
                return Err(FastMcpError::ToolExecution(format!(
                    "🛑 InterMCP Loop Breaker: Infinite loop detected! Tool '{}' was invoked with identical parameters {} times consecutively. Execution halted.",
                    tool_name, count
                )));
            }

            *last = Some((signature, count));
        }

        Ok(())
    }

    pub fn reset_signature(&self, _tool_name: &str, _arguments: &serde_json::Value) {
        if let Ok(mut last) = self.last_signature.write() {
            *last = None;
        }
    }
}
