use crate::error::FastMcpError;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct GuardrailPolicy {
    max_calls_per_minute: u32,
    loop_detection_threshold: u32,
    max_session_chars: Option<usize>,
    char_history: RwLock<VecDeque<(Instant, usize)>>,
    history: RwLock<HashMap<String, Vec<Instant>>>,
    last_signature: RwLock<Option<(String, u32)>>,
    prune_counter: AtomicU64,
}

impl GuardrailPolicy {
    pub fn new(max_calls_per_minute: u32, loop_detection_threshold: u32) -> Self {
        Self {
            max_calls_per_minute,
            loop_detection_threshold,
            max_session_chars: None,
            char_history: RwLock::new(VecDeque::new()),
            history: RwLock::new(HashMap::new()),
            last_signature: RwLock::new(None),
            prune_counter: AtomicU64::new(0),
        }
    }

    pub fn default_policy() -> Self {
        Self::new(60, 5)
    }

    pub fn with_token_budget(mut self, estimated_tokens: usize) -> Self {
        self.max_session_chars = Some(estimated_tokens.saturating_mul(4));
        self
    }

    pub fn with_char_budget(mut self, max_chars: usize) -> Self {
        self.max_session_chars = Some(max_chars);
        self
    }

    pub fn record_output(&self, text: &str) -> Result<(), FastMcpError> {
        if let Some(budget) = self.max_session_chars {
            let now = Instant::now();
            let window_start = now - Duration::from_secs(3600);

            let mut history = self.char_history.write();
            while let Some(&(time, _)) = history.front() {
                if time < window_start {
                    history.pop_front();
                } else {
                    break;
                }
            }

            let current_total: usize = history.iter().map(|(_, len)| *len).sum();
            let new_chars = text.len();

            if current_total.saturating_add(new_chars) > budget {
                return Err(FastMcpError::ToolExecution(format!(
                    "InterMCP Budget Sentinel: Cumulative output tokens ({} chars) reached session limit ({} chars). Execution paused to prevent runaway costs.",
                    current_total.saturating_add(new_chars), budget
                )));
            }

            history.push_back((now, new_chars));
        }
        Ok(())
    }

    pub fn estimated_tokens_used(&self) -> usize {
        let now = Instant::now();
        let window_start = now - Duration::from_secs(3600);
        let history = self.char_history.read();
        let active_chars: usize = history
            .iter()
            .filter(|(t, _)| *t >= window_start)
            .map(|(_, len)| *len)
            .sum();
        active_chars / 4
    }

    pub fn check_call(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), FastMcpError> {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        let count = self.prune_counter.fetch_add(1, Ordering::Relaxed);
        let mut hist = self.history.write();

        if count % 100 == 0 {
            hist.retain(|_, timestamps| {
                timestamps.retain(|&t| t > one_minute_ago);
                !timestamps.is_empty()
            });
        }

        let timestamps = hist.entry(tool_name.to_string()).or_default();
        timestamps.retain(|&t| t > one_minute_ago);

        if timestamps.len() >= self.max_calls_per_minute as usize {
            return Err(FastMcpError::ToolExecution(format!(
                "InterMCP Guardrail Triggered: Tool '{}' exceeded rate limit of {} calls/min. Execution paused to protect API budget.",
                tool_name, self.max_calls_per_minute
            )));
        }

        timestamps.push(now);

        let arg_hash = compute_normalized_arguments_hash(arguments);
        let signature = format!("{}:{}", tool_name, arg_hash);
        let mut last = self.last_signature.write();
        let consecutive_count = match &*last {
            Some((prev_sig, c)) if prev_sig == &signature => c + 1,
            _ => 1,
        };

        if consecutive_count > self.loop_detection_threshold {
            return Err(FastMcpError::ToolExecution(format!(
                "InterMCP Loop Breaker: Infinite loop detected! Tool '{}' was invoked with identical parameters {} times consecutively. Execution halted.",
                tool_name, consecutive_count
            )));
        }

        *last = Some((signature, consecutive_count));

        Ok(())
    }

    pub fn reset_signature(&self, _tool_name: &str, _arguments: &serde_json::Value) {
        *self.last_signature.write() = None;
    }

    pub fn reset(&self) {
        self.history.write().clear();
        self.char_history.write().clear();
        *self.last_signature.write() = None;
    }
}

fn normalize_value_for_loop_detection(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(s.trim().to_lowercase()),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(normalize_value_for_loop_detection).collect())
        }
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut normalized_map = serde_json::Map::new();
            for (k, v) in entries {
                normalized_map.insert(
                    k.trim().to_lowercase(),
                    normalize_value_for_loop_detection(v),
                );
            }
            serde_json::Value::Object(normalized_map)
        }
        other => other.clone(),
    }
}

pub fn compute_normalized_arguments_hash(value: &serde_json::Value) -> String {
    let normalized = normalize_value_for_loop_detection(value);
    match crate::receipts::hash_canonical_json(&normalized) {
        Ok(h) => h,
        Err(_) => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(normalized.to_string().as_bytes());
            format!("{:x}", hasher.finalize())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalized_arguments_hash_ignores_whitespace_and_case() {
        let args1 = json!({
            "command": "  GIT PUSH origin main  ",
            "flags": ["  --FORCE  "]
        });
        let args2 = json!({
            "command": "git push origin main",
            "flags": ["--force"]
        });

        let hash1 = compute_normalized_arguments_hash(&args1);
        let hash2 = compute_normalized_arguments_hash(&args2);
        assert_eq!(
            hash1, hash2,
            "Normalized arguments hashes must match despite whitespace/case differences"
        );

        let policy = GuardrailPolicy::new(100, 2);
        policy.check_call("cmd_run", &args1).unwrap();
        policy.check_call("cmd_run", &args2).unwrap();
        let loop_err = policy.check_call("cmd_run", &args1).unwrap_err();
        assert!(loop_err.to_string().contains("Infinite loop detected"));
    }
}
