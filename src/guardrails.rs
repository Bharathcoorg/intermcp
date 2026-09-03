use crate::error::FastMcpError;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Autonomous AI Agent Loop Breaker & Cost Guardrail
/// Prevents runaway autonomous agents from executing infinite loops or draining API budgets.
pub struct GuardrailPolicy {
    max_calls_per_minute: u32,
    loop_detection_threshold: u32,
    history: RwLock<HashMap<String, Vec<Instant>>>,
    recent_signatures: RwLock<HashMap<String, u32>>,
}

impl GuardrailPolicy {
    pub fn new(max_calls_per_minute: u32, loop_detection_threshold: u32) -> Self {
        Self {
            max_calls_per_minute,
            loop_detection_threshold,
            history: RwLock::new(HashMap::new()),
            recent_signatures: RwLock::new(HashMap::new()),
        }
    }

    pub fn default_policy() -> Self {
        Self::new(60, 5) // 60 calls/min, max 5 consecutive identical calls
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

        // 2. Infinite loop detection (repeated identical signatures)
        let signature = format!("{}:{}", tool_name, arguments);
        if let Ok(mut sigs) = self.recent_signatures.write() {
            let count = sigs.entry(signature.clone()).or_insert(0);
            *count += 1;

            if *count > self.loop_detection_threshold {
                return Err(FastMcpError::ToolExecution(format!(
                    "🛑 InterMCP Loop Breaker: Infinite loop detected! Tool '{}' was invoked with identical parameters {} times consecutively. Execution halted.",
                    tool_name, count
                )));
            }
        }

        Ok(())
    }

    pub fn reset_signature(&self, tool_name: &str, arguments: &serde_json::Value) {
        let signature = format!("{}:{}", tool_name, arguments);
        if let Ok(mut sigs) = self.recent_signatures.write() {
            sigs.remove(&signature);
        }
    }
}
