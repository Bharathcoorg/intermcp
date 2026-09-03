use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::error::FastMcpError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingActionSummary {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
    pub remaining_secs: u64,
}

struct PendingEntry {
    tool: String,
    arguments: Value,
    expires_at: Instant,
    sender: oneshot::Sender<bool>,
}

#[derive(Clone)]
pub struct TimeLockedVault {
    protected_tools: Vec<String>,
    window: Duration,
    pending: Arc<RwLock<HashMap<String, PendingEntry>>>,
    counter: Arc<AtomicU64>,
}

impl TimeLockedVault {
    pub fn new(protected_tools: Vec<String>, window_secs: u64) -> Self {
        Self {
            protected_tools,
            window: Duration::from_secs(window_secs),
            pending: Arc::new(RwLock::new(HashMap::new())),
            counter: Arc::new(AtomicU64::new(1001)),
        }
    }

    pub fn is_protected(&self, tool_name: &str) -> bool {
        self.protected_tools
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tool_name))
    }

    pub async fn check_or_wait(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<bool, FastMcpError> {
        if !self.is_protected(tool_name) {
            return Ok(true);
        }

        let num = self.counter.fetch_add(1, Ordering::SeqCst);
        let id = format!("{:x}", num);
        let expires_at = Instant::now() + self.window;

        let (tx, rx) = oneshot::channel();

        {
            let mut guard = self.pending.write();
            guard.insert(
                id.clone(),
                PendingEntry {
                    tool: tool_name.to_string(),
                    arguments: arguments.clone(),
                    expires_at,
                    sender: tx,
                },
            );
        }

        warn!(
            "⏳ TIME-LOCKED VAULT: Approval required for tool '{}' [Approval ID: {}]. Waiting up to {}s.",
            tool_name,
            id,
            self.window.as_secs()
        );

        match tokio::time::timeout(self.window, rx).await {
            Ok(Ok(approved)) => {
                self.pending.write().remove(&id);
                if approved {
                    info!("✅ Tool '{}' [ID: {}] APPROVED by supervisor", tool_name, id);
                    Ok(true)
                } else {
                    warn!("❌ Tool '{}' [ID: {}] REJECTED by supervisor", tool_name, id);
                    Ok(false)
                }
            }
            Ok(Err(_)) => {
                self.pending.write().remove(&id);
                Ok(false)
            }
            Err(_) => {
                self.pending.write().remove(&id);
                warn!(
                    "⌛ Tool '{}' [ID: {}] TIMED OUT after {}s without approval",
                    tool_name,
                    id,
                    self.window.as_secs()
                );
                Ok(false)
            }
        }
    }

    pub fn approve(&self, id: &str) -> bool {
        if let Some(entry) = self.pending.write().remove(id) {
            let _ = entry.sender.send(true);
            true
        } else {
            false
        }
    }

    pub fn reject(&self, id: &str) -> bool {
        if let Some(entry) = self.pending.write().remove(id) {
            let _ = entry.sender.send(false);
            true
        } else {
            false
        }
    }

    pub fn list_pending(&self) -> Vec<PendingActionSummary> {
        let now = Instant::now();
        let guard = self.pending.read();
        guard
            .iter()
            .filter(|(_, entry)| entry.expires_at > now)
            .map(|(id, entry)| PendingActionSummary {
                id: id.clone(),
                tool: entry.tool.clone(),
                arguments: entry.arguments.clone(),
                remaining_secs: entry.expires_at.duration_since(now).as_secs(),
            })
            .collect()
    }
}
