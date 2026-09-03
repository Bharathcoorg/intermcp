use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct CacheEntry {
    result: Value,
    expires_at: Instant,
}

/// High-speed in-memory LRU/TTL micro-cache for idempotent MCP tool invocations.
/// Prevents AI agents from burning tokens and latency on duplicate read-only queries.
pub struct ToolCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    default_ttl: Duration,
    hits: RwLock<u64>,
    misses: RwLock<u64>,
}

impl ToolCache {
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            default_ttl,
            hits: RwLock::new(0),
            misses: RwLock::new(0),
        }
    }

    pub fn compute_key(tool_name: &str, arguments: &Value) -> String {
        format!("{}:{}", tool_name, arguments)
    }

    pub fn get(&self, tool_name: &str, arguments: &Value) -> Option<Value> {
        let key = Self::compute_key(tool_name, arguments);
        let now = Instant::now();

        let read_guard = self.entries.read().ok()?;
        if let Some(entry) = read_guard.get(&key) {
            if entry.expires_at > now {
                if let Ok(mut hits) = self.hits.write() {
                    *hits += 1;
                }
                return Some(entry.result.clone());
            }
        }

        drop(read_guard);
        if let Ok(mut misses) = self.misses.write() {
            *misses += 1;
        }
        None
    }

    pub fn set(&self, tool_name: &str, arguments: &Value, result: Value, ttl: Option<Duration>) {
        let key = Self::compute_key(tool_name, arguments);
        let expires_at = Instant::now() + ttl.unwrap_or(self.default_ttl);

        if let Ok(mut write_guard) = self.entries.write() {
            // Prune expired entries if cache grows large
            if write_guard.len() > 1000 {
                let now = Instant::now();
                write_guard.retain(|_, v| v.expires_at > now);
            }

            write_guard.insert(key, CacheEntry { result, expires_at });
        }
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        let hits = self.hits.read().map(|h| *h).unwrap_or(0);
        let misses = self.misses.read().map(|m| *m).unwrap_or(0);
        let count = self.entries.read().map(|e| e.len()).unwrap_or(0);
        (hits, misses, count)
    }
}
