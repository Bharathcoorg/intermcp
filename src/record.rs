use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::FastMcpError;
use crate::server::Server;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFrame {
    pub v: u32,
    pub ts: u64,
    pub dir: FrameDirection,
    pub payload: Value,
}

#[derive(Clone)]
pub struct SessionRecorder {
    writer: Arc<Mutex<BufWriter<File>>>,
}

impl SessionRecorder {
    pub fn new(path: &Path) -> Result<Self, FastMcpError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(FastMcpError::Io)?;

        Ok(Self {
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
        })
    }

    pub fn record(&self, dir: FrameDirection, payload: &Value) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let frame = SessionFrame {
            v: 1,
            ts,
            dir,
            payload: payload.clone(),
        };

        if let Ok(serialized) = serde_json::to_string(&frame) {
            let mut guard = self.writer.lock();
            let _ = writeln!(guard, "{}", serialized);
            let _ = guard.flush();
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub total_calls: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub errors: Vec<String>,
}

pub struct SessionReplayer;

impl SessionReplayer {
    pub async fn replay(path: &Path, server: &Server) -> Result<ReplaySummary, FastMcpError> {
        let file = File::open(path).map_err(FastMcpError::Io)?;
        let reader = BufReader::new(file);

        let mut summary = ReplaySummary::default();
        let mut expected_outbound: Option<Value> = None;

        for line_res in reader.lines() {
            let line = line_res.map_err(FastMcpError::Io)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let frame: SessionFrame =
                serde_json::from_str(trimmed).map_err(FastMcpError::Serialization)?;

            match frame.dir {
                FrameDirection::Inbound => {
                    summary.total_calls += 1;
                    let raw_str = frame.payload.to_string();
                    let actual_resp_str = server.handle_raw_message(&raw_str).await;

                    if let Some(expected) = expected_outbound.take() {
                        if let Some(actual_str) = actual_resp_str {
                            if let Ok(actual_val) = serde_json::from_str::<Value>(&actual_str) {
                                if Self::responses_match(&expected, &actual_val) {
                                    summary.matched += 1;
                                } else {
                                    summary.mismatched += 1;
                                    summary.errors.push(format!(
                                        "Mismatch on call {}: expected {:?}, got {:?}",
                                        summary.total_calls, expected, actual_val
                                    ));
                                }
                            } else {
                                summary.mismatched += 1;
                            }
                        } else {
                            summary.mismatched += 1;
                        }
                    } else if actual_resp_str.is_some() {
                        summary.matched += 1;
                    }
                }
                FrameDirection::Outbound => {
                    expected_outbound = Some(frame.payload);
                }
            }
        }

        Ok(summary)
    }

    fn responses_match(expected: &Value, actual: &Value) -> bool {
        let mut exp_clean = expected.clone();
        let mut act_clean = actual.clone();

        if let Some(obj) = exp_clean.as_object_mut() {
            obj.remove("id");
        }
        if let Some(obj) = act_clean.as_object_mut() {
            obj.remove("id");
        }

        exp_clean == act_clean
    }
}
