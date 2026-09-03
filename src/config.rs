use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    #[serde(default)]
    pub sensitive_files: Vec<String>,
    #[serde(default)]
    pub sensitive_keywords: Vec<String>,
    pub rate_limit: Option<u32>,
    pub token_budget: Option<usize>,
    #[serde(default)]
    pub shell_allowlist: Vec<String>,
    #[serde(default)]
    pub shell_denylist: Vec<String>,
    pub cache_max_bytes: Option<usize>,
    pub http_max_conns: Option<usize>,
}

impl PolicyConfig {
    pub fn load_from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let cfg: Self = serde_json::from_str(&content)?;
            Ok(cfg)
        } else {
            let cfg: Self = toml::from_str(&content)?;
            Ok(cfg)
        }
    }
}
