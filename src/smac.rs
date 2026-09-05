use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::FastMcpError;

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmacEntry {
    pub index: u64,
    pub ts: u64,
    pub tool: String,
    pub prev_hash: String,
    pub req_hash: String,
    pub resp_hash: String,
    pub hash: String,
}

pub struct SmacLogger {
    writer: Arc<RwLock<BufWriter<File>>>,
    last_hash: Arc<RwLock<String>>,
    counter: AtomicU64,
}

impl SmacLogger {
    pub fn new(path: &Path) -> Result<Self, FastMcpError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(FastMcpError::Io)?;

        let mut last_hash = GENESIS_HASH.to_string();
        let mut count = 0;

        if let Ok(existing) = File::open(path) {
            let reader = BufReader::new(existing);
            for line in reader.lines().map_while(Result::ok) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Ok(entry) = serde_json::from_str::<SmacEntry>(trimmed) {
                        last_hash = entry.hash;
                        count = entry.index + 1;
                    }
                }
            }
        }

        Ok(Self {
            writer: Arc::new(RwLock::new(BufWriter::new(file))),
            last_hash: Arc::new(RwLock::new(last_hash)),
            counter: AtomicU64::new(count),
        })
    }

    pub fn hash_value(v: &Value) -> String {
        let canonical = crate::receipts::canonicalize_json(v)
            .unwrap_or_else(|_| serde_json::to_string(v).unwrap_or_default().into_bytes());
        let mut hasher = Sha256::new();
        hasher.update(&canonical);
        format!("{:x}", hasher.finalize())
    }

    pub fn compute_entry_hash(
        prev: &str,
        index: u64,
        tool: &str,
        req_hash: &str,
        resp_hash: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prev.as_bytes());
        hasher.update(index.to_string().as_bytes());
        hasher.update(tool.as_bytes());
        hasher.update(req_hash.as_bytes());
        hasher.update(resp_hash.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn record(&self, tool: &str, args: &Value, result: &Value) {
        let index = self.counter.fetch_add(1, Ordering::SeqCst);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let req_hash = Self::hash_value(args);
        let resp_hash = Self::hash_value(result);

        let mut last_guard = self.last_hash.write();
        let prev_hash = last_guard.clone();
        let entry_hash = Self::compute_entry_hash(&prev_hash, index, tool, &req_hash, &resp_hash);

        let entry = SmacEntry {
            index,
            ts,
            tool: tool.to_string(),
            prev_hash,
            req_hash,
            resp_hash,
            hash: entry_hash.clone(),
        };

        *last_guard = entry_hash;

        if let Ok(serialized) = serde_json::to_string(&entry) {
            let mut writer_guard = self.writer.write();
            let _ = writeln!(writer_guard, "{}", serialized);
            let _ = writer_guard.flush();
        }
    }
}

pub fn verify_smac_log(path: &Path) -> Result<usize, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open log: {}", e))?;
    let reader = BufReader::new(file);

    let mut prev_expected = GENESIS_HASH.to_string();
    let mut verified_count = 0;

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res.map_err(|e| format!("Read error at line {}: {}", line_idx + 1, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry: SmacEntry = serde_json::from_str(trimmed)
            .map_err(|e| format!("JSON parse error at line {}: {}", line_idx + 1, e))?;

        if entry.prev_hash != prev_expected {
            return Err(format!(
                "Chain broken at entry {} (line {}): expected prev_hash {}, found {}",
                entry.index,
                line_idx + 1,
                prev_expected,
                entry.prev_hash
            ));
        }

        let expected_hash = SmacLogger::compute_entry_hash(
            &entry.prev_hash,
            entry.index,
            &entry.tool,
            &entry.req_hash,
            &entry.resp_hash,
        );

        if entry.hash != expected_hash {
            return Err(format!(
                "Tampering detected at entry {} (line {}): recomputed hash {}, record claims {}",
                entry.index,
                line_idx + 1,
                expected_hash,
                entry.hash
            ));
        }

        prev_expected = entry.hash;
        verified_count += 1;
    }

    Ok(verified_count)
}
