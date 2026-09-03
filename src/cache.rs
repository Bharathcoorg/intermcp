use parking_lot::RwLock;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct CacheEntry {
    result: Value,
    size_bytes: usize,
    last_accessed: Instant,
    expires_at: Instant,
}

pub struct ToolCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    access_index: RwLock<BTreeMap<Instant, String>>,
    default_ttl: Duration,
    max_bytes: usize,
    max_entries: usize,
    bytes_used: RwLock<usize>,
    hits: RwLock<u64>,
    misses: RwLock<u64>,
}

impl ToolCache {
    pub fn new(default_ttl: Duration) -> Self {
        Self::with_max_bytes(default_ttl, 50 * 1024 * 1024)
    }

    pub fn with_max_bytes(default_ttl: Duration, max_bytes: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            access_index: RwLock::new(BTreeMap::new()),
            default_ttl,
            max_bytes,
            max_entries: 1000,
            bytes_used: RwLock::new(0),
            hits: RwLock::new(0),
            misses: RwLock::new(0),
        }
    }

    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    pub fn compute_key(tool_name: &str, arguments: &Value) -> String {
        format!("{}:{}", tool_name, arguments)
    }

    pub fn estimated_size(v: &Value) -> usize {
        match v {
            Value::Null => 8,
            Value::Bool(_) => 8,
            Value::Number(_) => 16,
            Value::String(s) => s.len() + 24,
            Value::Array(arr) => 24 + arr.iter().map(Self::estimated_size).sum::<usize>(),
            Value::Object(map) => {
                24 + map
                    .iter()
                    .map(|(k, val)| k.len() + 24 + Self::estimated_size(val))
                    .sum::<usize>()
            }
        }
    }

    pub fn get(&self, tool_name: &str, arguments: &Value) -> Option<Value> {
        let key = Self::compute_key(tool_name, arguments);
        let now = Instant::now();

        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&key) {
            if entry.expires_at > now {
                let old_access = entry.last_accessed;
                entry.last_accessed = now;

                let mut idx = self.access_index.write();
                idx.remove(&old_access);
                idx.insert(now, key);

                *self.hits.write() += 1;
                return Some(entry.result.clone());
            }
        }

        *self.misses.write() += 1;
        None
    }

    pub fn set(&self, tool_name: &str, arguments: &Value, result: Value, ttl: Option<Duration>) {
        let key = Self::compute_key(tool_name, arguments);
        let now = Instant::now();
        let expires_at = now + ttl.unwrap_or(self.default_ttl);
        let size_bytes = Self::estimated_size(&result) + key.len();

        if size_bytes > self.max_bytes {
            return;
        }

        let mut entries = self.entries.write();
        let mut idx = self.access_index.write();
        let mut bytes_used = self.bytes_used.write();

        if let Some(existing) = entries.remove(&key) {
            idx.remove(&existing.last_accessed);
            *bytes_used = bytes_used.saturating_sub(existing.size_bytes);
        }

        while *bytes_used + size_bytes > self.max_bytes || entries.len() >= self.max_entries {
            if let Some((&oldest_time, _)) = idx.iter().next() {
                if let Some(evicted_key) = idx.remove(&oldest_time) {
                    if let Some(evicted) = entries.remove(&evicted_key) {
                        *bytes_used = bytes_used.saturating_sub(evicted.size_bytes);
                    }
                }
            } else {
                break;
            }
        }

        if *bytes_used + size_bytes <= self.max_bytes && entries.len() < self.max_entries {
            *bytes_used += size_bytes;
            entries.insert(
                key.clone(),
                CacheEntry {
                    result,
                    size_bytes,
                    last_accessed: now,
                    expires_at,
                },
            );
            idx.insert(now, key);
        }
    }

    pub fn bytes_used(&self) -> usize {
        *self.bytes_used.read()
    }

    pub fn stats(&self) -> (u64, u64, usize) {
        let hits = *self.hits.read();
        let misses = *self.misses.read();
        let count = self.entries.read().len();
        (hits, misses, count)
    }
}
