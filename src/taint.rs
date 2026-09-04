use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Sensitivity and Trust Classification for context items and tool parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensitivityLabel {
    /// Public information, safe for external transmission and unredacted logging
    Public = 0,
    /// Internal project content, safe for local agent reasoning but not public leaks
    Internal = 1,
    /// Sensitive credentials, secrets, and private business data
    Confidential = 2,
    /// Untrusted data from external sources (web scraping, third-party MCP upstreams)
    /// Cannot flow directly into privileged sinks (shell execution, code writing)
    Untrusted = 3,
}

impl SensitivityLabel {
    pub fn is_untrusted(&self) -> bool {
        matches!(self, Self::Untrusted)
    }

    pub fn is_confidential(&self) -> bool {
        matches!(self, Self::Confidential)
    }
}

/// Permissible sink types in an AI agent architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SinkCapability {
    /// Read-only inspection (SafeFS read, git status, system info)
    ReadOnlyInspection,
    /// File mutation (writing source files)
    FileMutation,
    /// Privileged shell execution (running bash, PowerShell, system binaries)
    PrivilegedExecution,
    /// Network egress (HTTP requests, external tool dispatch)
    NetworkEgress,
}

#[derive(Debug, thiserror::Error)]
pub enum TaintViolation {
    #[error("Taint Flow Violation: Untrusted data cannot flow into privileged sink '{0:?}' without sanitization or supervisor approval")]
    UntrustedToPrivilegedSink(SinkCapability),
    #[error("Confidentiality Flow Violation: Confidential data '{0}' cannot egress to external network destination")]
    ConfidentialEgressBlocked(String),
}

/// Tracks taint metadata across active sessions and tool invocations
pub struct TaintTracker {
    /// Maps session/context item IDs to their assigned sensitivity labels
    item_labels: Arc<RwLock<HashMap<String, SensitivityLabel>>>,
    violations: AtomicU64,
}

impl Default for TaintTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TaintTracker {
    pub fn new() -> Self {
        Self {
            item_labels: Arc::new(RwLock::new(HashMap::new())),
            violations: AtomicU64::new(0),
        }
    }

    /// Assign a sensitivity label to a resource or context item
    pub fn tag_item(&self, item_id: &str, label: SensitivityLabel) {
        self.item_labels.write().insert(item_id.to_string(), label);
    }

    /// Retrieve the sensitivity label for a context item
    pub fn get_label(&self, item_id: &str) -> SensitivityLabel {
        self.item_labels
            .read()
            .get(item_id)
            .copied()
            .unwrap_or(SensitivityLabel::Public)
    }

    /// Validate whether data with given label is permitted to flow into a target sink
    pub fn check_flow(
        &self,
        source_label: SensitivityLabel,
        target_sink: SinkCapability,
    ) -> Result<(), TaintViolation> {
        match (source_label, target_sink) {
            (SensitivityLabel::Untrusted, SinkCapability::PrivilegedExecution) => {
                self.violations.fetch_add(1, Ordering::Relaxed);
                Err(TaintViolation::UntrustedToPrivilegedSink(target_sink))
            }
            (SensitivityLabel::Confidential, SinkCapability::NetworkEgress) => {
                self.violations.fetch_add(1, Ordering::Relaxed);
                Err(TaintViolation::ConfidentialEgressBlocked(
                    "Network egress destination rejected for confidential payload".into(),
                ))
            }
            _ => Ok(()),
        }
    }

    /// Inspect a JSON argument payload for untrusted taint markers
    pub fn scan_json_arguments(
        &self,
        args: &Value,
        sink: SinkCapability,
    ) -> Result<(), TaintViolation> {
        if let Some(obj) = args.as_object() {
            // Check if arguments contain explicit taint metadata or untrusted flags
            if let Some(taint_val) = obj.get("_taint") {
                if taint_val.as_str() == Some("untrusted") {
                    return self.check_flow(SensitivityLabel::Untrusted, sink);
                }
            }
        }
        Ok(())
    }

    pub fn total_violations(&self) -> u64 {
        self.violations.load(Ordering::Relaxed)
    }
}
