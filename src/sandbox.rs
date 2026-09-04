use crate::error::FastMcpError;
use std::path::{Component, Path, PathBuf};

const DEFAULT_SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    ".netrc",
    ".pgpass",
    ".bash_history",
    ".zsh_history",
    "kubeconfig",
    "credentials.json",
    "service_account.json",
    "vault.json",
    ".npmrc",
    "secrets.json",
    "secret.json",
    "token.json",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];

const DEFAULT_SENSITIVE_KEYWORDS: &[&str] = &[
    "secret",
    "token",
    "key",
    "password",
    "credential",
    "mnemonic",
    "seed",
    "wallet",
    "private",
];

const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key", "pkcs12", "pfx"];

#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    allowed_roots: Vec<PathBuf>,
    enabled: bool,
    shield_secrets: bool,
    custom_sensitive_files: Vec<String>,
    custom_sensitive_keywords: Vec<String>,
}

impl SandboxPolicy {
    pub fn unrestricted() -> Self {
        Self {
            allowed_roots: Vec::new(),
            enabled: false,
            shield_secrets: true,
            custom_sensitive_files: Vec::new(),
            custom_sensitive_keywords: Vec::new(),
        }
    }

    pub fn new(roots: Vec<PathBuf>) -> Self {
        let canonical_roots: Vec<PathBuf> = roots
            .into_iter()
            .filter_map(|p| dunce::canonicalize(&p).ok())
            .collect();

        Self {
            allowed_roots: canonical_roots,
            enabled: true,
            shield_secrets: true,
            custom_sensitive_files: Vec::new(),
            custom_sensitive_keywords: Vec::new(),
        }
    }

    pub fn with_secret_shield(mut self, enabled: bool) -> Self {
        self.shield_secrets = enabled;
        self
    }

    pub fn with_additional_sensitive_files(mut self, files: Vec<String>) -> Self {
        self.custom_sensitive_files.extend(files);
        self
    }

    pub fn with_additional_sensitive_keywords(mut self, keywords: Vec<String>) -> Self {
        self.custom_sensitive_keywords.extend(keywords);
        self
    }

    pub fn is_reserved_device_name(name: &str) -> bool {
        let stem = match name.split('.').next() {
            Some(s) => s.to_ascii_uppercase(),
            None => return false,
        };
        matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "CONIN$"
                | "CONOUT$"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
    }

    pub fn is_windows_short_name(name: &str) -> bool {
        if let Some(pos) = name.find('~') {
            let rest = &name[pos + 1..];
            let digits = rest.split('.').next().unwrap_or("");
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
        } else {
            false
        }
    }

    pub fn is_sensitive_path(&self, path: &Path) -> bool {
        Self::check_sensitive_path(
            path,
            &self.custom_sensitive_files,
            &self.custom_sensitive_keywords,
        )
    }

    pub fn is_sensitive_path_default(path: &Path) -> bool {
        Self::check_sensitive_path(path, &[], &[])
    }

    pub fn check_sensitive_path(
        path: &Path,
        custom_files: &[String],
        custom_keywords: &[String],
    ) -> bool {
        let path_str = path.to_string_lossy().to_lowercase().replace('\\', "/");

        if path_str.contains(".docker/config.json") || path_str.contains("gcloud/credentials.db") {
            return true;
        }

        for component in path.components() {
            if let Component::Normal(c) = component {
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

        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            let lower_name = file_name.to_lowercase();
            for sensitive in DEFAULT_SENSITIVE_FILES {
                if lower_name == *sensitive || lower_name.starts_with(".env.") {
                    return true;
                }
            }
            for custom in custom_files {
                if lower_name == custom.to_lowercase() {
                    return true;
                }
            }

            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let lower_stem = stem.to_lowercase();
                for kw in DEFAULT_SENSITIVE_KEYWORDS {
                    if lower_stem.contains(kw) {
                        return true;
                    }
                }
                for kw in custom_keywords {
                    if lower_stem.contains(&kw.to_lowercase()) {
                        return true;
                    }
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
        let raw_str = requested.to_string_lossy();
        if raw_str.starts_with(r"\\?\") || raw_str.starts_with("//?/") {
            return Err(FastMcpError::ToolExecution(
                "Access denied: Verbatim UNC prefixes (\\\\?\\) are prohibited.".into(),
            ));
        }

        for comp in requested.components() {
            if let Component::Normal(os_str) = comp {
                let s = os_str.to_string_lossy();
                if Self::is_reserved_device_name(&s) {
                    return Err(FastMcpError::ToolExecution(format!(
                        "Access denied: '{}' is a reserved NTFS device name.",
                        s
                    )));
                }
                if Self::is_windows_short_name(&s) {
                    return Err(FastMcpError::ToolExecution(format!(
                        "Access denied: 8.3 short name alias '{}' is prohibited.",
                        s
                    )));
                }
            }
        }

        if self.shield_secrets && self.is_sensitive_path(requested) {
            return Err(FastMcpError::ToolExecution(format!(
                "SafeFS Secret Shield: Access denied to '{}'. Credential and sensitive files are protected.",
                requested.display()
            )));
        }

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

        let mut ancestor = curr;
        loop {
            if ancestor.exists() {
                if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
                    if meta.file_type().is_symlink() {
                        return Err(FastMcpError::ToolExecution(format!(
                            "SafeFS Violation: Symlink detected in path component '{}'. Symlinks are prohibited.",
                            ancestor.display()
                        )));
                    }
                }
            }
            match ancestor.parent() {
                Some(p) if !p.as_os_str().is_empty() && p != ancestor => ancestor = p,
                _ => break,
            }
        }

        let base = if curr.exists() {
            dunce::canonicalize(curr).map_err(|e| {
                FastMcpError::ToolExecution(format!("Path canonicalization failed: {}", e))
            })?
        } else {
            dunce::canonicalize(Path::new(".")).map_err(|e| {
                FastMcpError::ToolExecution(format!("Failed to resolve base directory: {}", e))
            })?
        };

        let mut target_to_check = base;
        for comp in suffix_components.into_iter().rev() {
            target_to_check.push(comp);
        }

        if target_to_check.exists() {
            if let Ok(meta) = std::fs::symlink_metadata(&target_to_check) {
                if meta.file_type().is_symlink() {
                    return Err(FastMcpError::ToolExecution(format!(
                        "SafeFS Violation: Symlink detected at target '{}'. Symlinks are prohibited.",
                        target_to_check.display()
                    )));
                }
            }
            target_to_check = dunce::canonicalize(&target_to_check).map_err(|e| {
                FastMcpError::ToolExecution(format!("Canonicalization failed: {}", e))
            })?;
        }

        if self.shield_secrets && self.is_sensitive_path(&target_to_check) {
            return Err(FastMcpError::ToolExecution(format!(
                "SafeFS Secret Shield: Access denied to '{}'.",
                target_to_check.display()
            )));
        }

        if !self.enabled || self.allowed_roots.is_empty() {
            return Ok(target_to_check);
        }

        for root in &self.allowed_roots {
            if target_to_check.ancestors().any(|a| a == root) {
                return Ok(target_to_check);
            }
        }

        Err(FastMcpError::ToolExecution(format!(
            "SafeFS Security Violation: Target '{}' escapes authorized directories: {:?}",
            target_to_check.display(),
            self.allowed_roots
        )))
    }
}
