use crate::error::FastMcpError;
use std::path::{Path, PathBuf};

/// Sensitive file patterns that should NEVER be accessed by AI tools to prevent credential exfiltration.
const SENSITIVE_FILE_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    ".npmrc",
    "credentials.json",
    "service_account.json",
    "vault.json",
];

const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key", "pkcs12", "pfx"];

/// SafeFS Security Sandbox: Guarantees AI tools cannot escape designated project directories
/// and protects sensitive credential files (.env, id_rsa, .pem) from exfiltration.
#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    allowed_roots: Vec<PathBuf>,
    enabled: bool,
    shield_secrets: bool,
}

impl SandboxPolicy {
    pub fn unrestricted() -> Self {
        Self {
            allowed_roots: Vec::new(),
            enabled: false,
            shield_secrets: true,
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
            shield_secrets: true,
        }
    }

    pub fn with_secret_shield(mut self, enabled: bool) -> Self {
        self.shield_secrets = enabled;
        self
    }

    /// Check if a path targets a credential, private key, or sensitive directory (.git, .ssh, .aws)
    pub fn is_sensitive_path(path: &Path) -> bool {
        // 1. Check for sensitive directory segments
        for component in path.components() {
            if let std::path::Component::Normal(c) = component {
                let segment = c.to_string_lossy().to_lowercase();
                if segment == ".ssh"
                    || segment == ".aws"
                    || segment == ".git"
                    || segment == ".gnupg"
                    || segment == ".docker"
                {
                    return true;
                }
            }
        }

        // 2. Check for sensitive file names and extensions
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            let lower = file_name.to_lowercase();
            for sensitive in SENSITIVE_FILE_NAMES {
                if lower == *sensitive || lower.starts_with(".env.") {
                    return true;
                }
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                for sensitive_ext in SENSITIVE_EXTENSIONS {
                    if ext_lower == *sensitive_ext {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn validate_path(&self, requested: &Path) -> Result<PathBuf, FastMcpError> {
        // 1. Secret Shield check: block sensitive credential files regardless of location
        if self.shield_secrets && Self::is_sensitive_path(requested) {
            return Err(FastMcpError::ToolExecution(format!(
                "🛡️ SafeFS Secret Shield: Access denied to '{}'. Credential files are protected from AI tool access.",
                requested.display()
            )));
        }

        if !self.enabled || self.allowed_roots.is_empty() {
            return Ok(requested.to_path_buf());
        }

        // 2. Walk up to find the deepest ancestor that exists on disk
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
