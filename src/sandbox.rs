use crate::error::FastMcpError;
use std::path::{Path, PathBuf};

/// SafeFS Security Sandbox: Guarantees AI tools cannot escape designated project directories
/// Prevents prompt injection attacks from exfiltrating ~/.ssh, ~/.aws, or system files.
#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    allowed_roots: Vec<PathBuf>,
    enabled: bool,
}

impl SandboxPolicy {
    pub fn unrestricted() -> Self {
        Self {
            allowed_roots: Vec::new(),
            enabled: false,
        }
    }

    pub fn new(roots: Vec<PathBuf>) -> Self {
        let canonical_roots: Vec<PathBuf> = roots
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        Self {
            allowed_roots: canonical_roots,
            enabled: true,
        }
    }

    pub fn validate_path(&self, requested: &Path) -> Result<PathBuf, FastMcpError> {
        if !self.enabled || self.allowed_roots.is_empty() {
            return Ok(requested.to_path_buf());
        }

        // Walk up to find the deepest ancestor that exists on disk
        let mut curr = requested;
        let mut suffix_components = Vec::new();

        while !curr.exists() {
            if let Some(name) = curr.file_name() {
                suffix_components.push(name);
            }
            match curr.parent() {
                Some(p) if !p.as_os_str().is_empty() => curr = p,
                _ => break,
            }
        }

        let base = if curr.exists() {
            curr.canonicalize().map_err(|e| {
                FastMcpError::ToolExecution(format!("Path canonicalization failed: {}", e))
            })?
        } else {
            Path::new(".").canonicalize().map_err(|e| {
                FastMcpError::ToolExecution(format!("Failed to resolve base directory: {}", e))
            })?
        };

        let mut target_to_check = base;
        for comp in suffix_components.into_iter().rev() {
            target_to_check.push(comp);
        }

        for root in &self.allowed_roots {
            if target_to_check.starts_with(root) {
                return Ok(target_to_check);
            }
        }

        Err(FastMcpError::ToolExecution(format!(
            "🛡️ SafeFS Security Violation: Access denied to path '{}'. Operation blocked outside allowed workspace boundaries.",
            requested.display()
        )))
    }
}
