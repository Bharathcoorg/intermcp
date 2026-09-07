use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
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

const MAX_PENDING: usize = 256;

struct CleanupGuard {
    token: CancellationToken,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

#[derive(Clone)]
pub struct TimeLockedVault {
    protected_tools: Vec<String>,
    window: Duration,
    pending: Arc<Mutex<HashMap<String, PendingEntry>>>,
    _cleanup_guard: Arc<CleanupGuard>,
}

impl TimeLockedVault {
    pub fn new(protected_tools: Vec<String>, window_secs: u64) -> Self {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let cancellation_token = CancellationToken::new();

        let pending_bg = Arc::clone(&pending);
        let cancel_bg = cancellation_token.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = cancel_bg.cancelled() => break,
                    _ = interval.tick() => {
                        let now = Instant::now();
                        pending_bg.lock().retain(|_, e: &mut PendingEntry| e.expires_at > now);
                    }
                }
            }
        });

        Self {
            protected_tools,
            window: Duration::from_secs(window_secs),
            pending,
            _cleanup_guard: Arc::new(CleanupGuard {
                token: cancellation_token,
            }),
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

        if self.pending.lock().len() >= MAX_PENDING {
            return Err(FastMcpError::SecurityViolation(
                "Vault saturated; refusing new pending actions".into(),
            ));
        }

        let mut token_bytes = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut token_bytes);
        let id: String = token_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let expires_at = Instant::now() + self.window;

        let (tx, rx) = oneshot::channel();

        {
            let mut guard = self.pending.lock();
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
            crate::server::redact_for_log(tool_name),
            id,
            self.window.as_secs()
        );

        match tokio::time::timeout(self.window, rx).await {
            Ok(Ok(approved)) => {
                if approved {
                    info!(
                        "✅ Tool '{}' [ID: {}] APPROVED by supervisor",
                        crate::server::redact_for_log(tool_name),
                        id
                    );
                    Ok(true)
                } else {
                    warn!(
                        "❌ Tool '{}' [ID: {}] REJECTED by supervisor",
                        crate::server::redact_for_log(tool_name),
                        id
                    );
                    Ok(false)
                }
            }
            Ok(Err(_)) => {
                self.pending.lock().remove(&id);
                Ok(false)
            }
            Err(_) => {
                self.pending.lock().remove(&id);
                warn!(
                    "⌛ Tool '{}' [ID: {}] TIMED OUT after {}s without approval",
                    crate::server::redact_for_log(tool_name),
                    id,
                    self.window.as_secs()
                );
                Ok(false)
            }
        }
    }

    pub fn approve(&self, id: &str) -> bool {
        let mut guard = self.pending.lock();
        if let Some(entry) = guard.remove(id) {
            if entry.sender.send(true).is_err() {
                warn!("Vault approve: receiver dropped for ID '{}'", id);
            }
            true
        } else {
            false
        }
    }

    pub fn reject(&self, id: &str) -> bool {
        let mut guard = self.pending.lock();
        if let Some(entry) = guard.remove(id) {
            if entry.sender.send(false).is_err() {
                warn!("Vault reject: receiver dropped for ID '{}'", id);
            }
            true
        } else {
            false
        }
    }

    pub fn list_pending(&self) -> Vec<PendingActionSummary> {
        let now = Instant::now();
        let guard = self.pending.lock();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vault_idempotent_approve() {
        let vault = TimeLockedVault::new(vec!["high_risk".into()], 30);
        let vault_clone = vault.clone();

        let handle = tokio::spawn(async move {
            vault_clone
                .check_or_wait("high_risk", &serde_json::json!({}))
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let pending = vault.list_pending();
        assert_eq!(pending.len(), 1);
        let id = &pending[0].id;

        assert!(vault.approve(id));
        assert!(!vault.approve(id));

        let res = handle.await.unwrap();
        assert!(res.unwrap());
    }

    #[tokio::test]
    async fn test_vault_pending_cap_saturation() {
        let vault = TimeLockedVault::new(vec!["high_risk".into()], 30);

        {
            let mut guard = vault.pending.lock();
            for i in 0..MAX_PENDING {
                let (tx, _rx) = oneshot::channel();
                guard.insert(
                    format!("pending-{}", i),
                    PendingEntry {
                        tool: "high_risk".to_string(),
                        arguments: serde_json::json!({}),
                        expires_at: Instant::now() + Duration::from_secs(60),
                        sender: tx,
                    },
                );
            }
        }

        let res = vault
            .check_or_wait("high_risk", &serde_json::json!({}))
            .await;
        assert!(res.is_err());
        match res {
            Err(FastMcpError::SecurityViolation(msg)) => {
                assert!(msg.contains("Vault saturated; refusing new pending actions"));
            }
            _ => panic!("Expected SecurityViolation on saturation"),
        }
    }

    #[tokio::test]
    async fn test_vault_expired_entry_cleaned_up() {
        let vault = TimeLockedVault::new(vec!["high_risk".into()], 30);

        {
            let (tx, _rx) = oneshot::channel();
            vault.pending.lock().insert(
                "expired-id".to_string(),
                PendingEntry {
                    tool: "high_risk".to_string(),
                    arguments: serde_json::json!({}),
                    expires_at: Instant::now() - Duration::from_secs(5),
                    sender: tx,
                },
            );
        }

        assert_eq!(vault.list_pending().len(), 0);

        vault
            .pending
            .lock()
            .retain(|_, e| e.expires_at > Instant::now());
        assert_eq!(vault.pending.lock().len(), 0);
    }
}
