use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Enforcement mode for the policy engine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    #[default]
    Enforcing,
    Permissive,
    AuditOnly,
}

/// Filesystem access permissions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    #[serde(default)]
    pub read_only: Vec<String>,
    #[serde(default)]
    pub read_write: Vec<String>,
    #[serde(default)]
    pub denied: Vec<String>,
}

/// Shell execution restrictions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellPolicy {
    #[serde(default)]
    pub allowed_binaries: Vec<String>,
    #[serde(default)]
    pub blocked_patterns: Vec<String>,
    #[serde(default)]
    pub require_approval: Vec<String>,
}

/// Operational resource and rate limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsPolicy {
    #[serde(default = "default_rate_limit")]
    pub max_calls_per_minute: u32,
    #[serde(default = "default_output_limit")]
    pub max_output_bytes: usize,
}

fn default_rate_limit() -> u32 {
    120
}

fn default_output_limit() -> usize {
    2 * 1024 * 1024 // 2 MB
}

impl Default for LimitsPolicy {
    fn default() -> Self {
        Self {
            max_calls_per_minute: default_rate_limit(),
            max_output_bytes: default_output_limit(),
        }
    }
}

/// Declarative Policy Configuration Document (TOML or JSON)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeclarativePolicy {
    #[serde(default)]
    pub mode: PolicyMode,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub shell: ShellPolicy,
    #[serde(default)]
    pub limits: LimitsPolicy,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyViolation {
    #[error("Filesystem access denied by policy: {0}")]
    FilesystemDenied(String),
    #[error("Shell binary '{0}' is not in the policy allowlist")]
    DisallowedBinary(String),
    #[error("Shell command matches blocked security pattern: {0}")]
    BlockedShellPattern(String),
    #[error("Rate limit exceeded for tool '{0}' ({1} calls/min limit)")]
    RateLimitExceeded(String, u32),
    #[error("Tool output exceeds configured limit of {0} bytes")]
    OutputLimitExceeded(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellPolicyDecision {
    Allow,
    RequireSupervisorApproval(String),
}

/// Sliding window rate limiter state
struct RateLimiter {
    timestamps: VecDeque<Instant>,
}

/// High-Performance Atomic Policy Engine & Runtime Gate
pub struct PolicyEngine {
    policy: DeclarativePolicy,
    rate_limits: Arc<RwLock<HashMap<String, RateLimiter>>>,
    violations_count: AtomicU64,
}

impl PolicyEngine {
    pub fn new(policy: DeclarativePolicy) -> Self {
        Self {
            policy,
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            violations_count: AtomicU64::new(0),
        }
    }

    /// Load and compile policy from TOML string
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        let policy: DeclarativePolicy = toml::from_str(content)?;
        Ok(Self::new(policy))
    }

    /// Load and compile policy from JSON string
    pub fn from_json(content: &str) -> Result<Self, serde_json::Error> {
        let policy: DeclarativePolicy = serde_json::from_str(content)?;
        Ok(Self::new(policy))
    }

    pub fn mode(&self) -> PolicyMode {
        self.policy.mode
    }

    pub fn total_violations(&self) -> u64 {
        self.violations_count.load(Ordering::Relaxed)
    }

    /// Evaluate filesystem read/write operation against path rules
    pub fn check_filesystem(&self, path: &Path, is_write: bool) -> Result<(), PolicyViolation> {
        let raw_str = path.to_string_lossy().to_lowercase().replace('\\', "/");
        let canonical_or_normalized = dunce::canonicalize(path).unwrap_or_else(|_| {
            let mut normalized = std::path::PathBuf::new();
            for comp in path.components() {
                match comp {
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        normalized.pop();
                    }
                    _ => normalized.push(comp.as_os_str()),
                }
            }
            normalized
        });
        let norm_str = canonical_or_normalized
            .to_string_lossy()
            .to_lowercase()
            .replace('\\', "/");

        // 1. Check denied patterns
        for pattern in &self.policy.filesystem.denied {
            let p_norm = pattern.to_lowercase().replace('\\', "/");
            let p_trimmed = p_norm.trim_end_matches('/');
            if raw_str.contains(&p_norm)
                || raw_str.ends_with(p_trimmed)
                || norm_str.contains(&p_norm)
                || norm_str.ends_with(p_trimmed)
            {
                self.record_violation();
                if self.policy.mode == PolicyMode::Enforcing {
                    return Err(PolicyViolation::FilesystemDenied(format!(
                        "Path '{}' matches denied pattern '{}'",
                        path.display(),
                        pattern
                    )));
                }
            }
        }

        let is_within_dir = |target: &Path, allowed_dir: &str| -> bool {
            let allowed_path = Path::new(allowed_dir);
            if target.ancestors().any(|a| a == allowed_path) {
                return true;
            }
            if let (Ok(c_target), Ok(c_allowed)) = (
                dunce::canonicalize(target),
                dunce::canonicalize(allowed_path),
            ) {
                if c_target.ancestors().any(|a| a == c_allowed) {
                    return true;
                }
            }
            false
        };

        // 2. Check write permissions if writing
        if is_write && !self.policy.filesystem.read_write.is_empty() {
            let allowed = self
                .policy
                .filesystem
                .read_write
                .iter()
                .any(|allowed_dir| is_within_dir(path, allowed_dir));

            if !allowed {
                self.record_violation();
                if self.policy.mode == PolicyMode::Enforcing {
                    return Err(PolicyViolation::FilesystemDenied(format!(
                        "Write access denied: path '{}' is not within any allowed read_write directory",
                        path.display()
                    )));
                }
            }
        }

        // 3. Check read permissions if read-only is restricted
        if !is_write && !self.policy.filesystem.read_only.is_empty() {
            let allowed = self
                .policy
                .filesystem
                .read_only
                .iter()
                .any(|allowed_dir| is_within_dir(path, allowed_dir))
                || self
                    .policy
                    .filesystem
                    .read_write
                    .iter()
                    .any(|allowed_dir| is_within_dir(path, allowed_dir));

            if !allowed {
                self.record_violation();
                if self.policy.mode == PolicyMode::Enforcing {
                    return Err(PolicyViolation::FilesystemDenied(format!(
                        "Read access denied: path '{}' is not within any allowed directory",
                        path.display()
                    )));
                }
            }
        }

        Ok(())
    }

    /// Evaluate shell command binary and argument safety
    pub fn check_shell(
        &self,
        binary: &str,
        raw_command: &str,
    ) -> Result<ShellPolicyDecision, PolicyViolation> {
        let binary_clean = binary.trim().to_lowercase();
        let cmd_lower = raw_command.to_lowercase();

        // 1. Binary allowlist check
        if !self.policy.shell.allowed_binaries.is_empty() {
            let allowed = self
                .policy
                .shell
                .allowed_binaries
                .iter()
                .any(|b| b.to_lowercase() == binary_clean);
            if !allowed {
                self.record_violation();
                if self.policy.mode == PolicyMode::Enforcing {
                    return Err(PolicyViolation::DisallowedBinary(binary.to_string()));
                }
            }
        }

        // 2. Blocked pattern check
        for pattern in &self.policy.shell.blocked_patterns {
            if cmd_lower.contains(&pattern.to_lowercase()) {
                self.record_violation();
                if self.policy.mode == PolicyMode::Enforcing {
                    return Err(PolicyViolation::BlockedShellPattern(pattern.clone()));
                }
            }
        }

        // 3. Supervisor approval check
        for approval_pattern in &self.policy.shell.require_approval {
            if cmd_lower.contains(&approval_pattern.to_lowercase()) {
                return Ok(ShellPolicyDecision::RequireSupervisorApproval(
                    approval_pattern.clone(),
                ));
            }
        }

        Ok(ShellPolicyDecision::Allow)
    }

    /// Check and enforce sliding-window rate limit for a tool
    pub fn check_rate_limit(&self, tool_name: &str) -> Result<(), PolicyViolation> {
        let max_calls = self.policy.limits.max_calls_per_minute;
        if max_calls == 0 {
            return Ok(());
        }

        let now = Instant::now();
        let mut limiters = self.rate_limits.write();
        let limiter = limiters
            .entry(tool_name.to_string())
            .or_insert_with(|| RateLimiter {
                timestamps: VecDeque::new(),
            });

        // Evict timestamps outside the 60-second sliding window
        while let Some(&front) = limiter.timestamps.front() {
            if now.duration_since(front).as_secs() >= 60 {
                limiter.timestamps.pop_front();
            } else {
                break;
            }
        }

        if limiter.timestamps.len() >= max_calls as usize {
            self.record_violation();
            if self.policy.mode == PolicyMode::Enforcing {
                return Err(PolicyViolation::RateLimitExceeded(
                    tool_name.to_string(),
                    max_calls,
                ));
            }
        } else {
            limiter.timestamps.push_back(now);
        }

        Ok(())
    }

    /// Check if output exceeds maximum byte limit
    pub fn check_output_size(&self, byte_len: usize) -> Result<(), PolicyViolation> {
        let max_bytes = self.policy.limits.max_output_bytes;
        if byte_len > max_bytes {
            self.record_violation();
            if self.policy.mode == PolicyMode::Enforcing {
                return Err(PolicyViolation::OutputLimitExceeded(max_bytes));
            }
        }
        Ok(())
    }

    fn record_violation(&self) {
        self.violations_count.fetch_add(1, Ordering::Relaxed);
    }
}
