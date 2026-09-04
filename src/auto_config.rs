use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct SetupResult {
    pub name: String,
    pub path: PathBuf,
    pub success: bool,
    pub message: String,
}

pub use configure_all_ides as auto_configure_all_ides;

pub fn configure_all_ides(binary_path: &str) -> Vec<SetupResult> {
    let mut results = Vec::new();

    // 1. Claude Desktop
    let claude_path = get_claude_desktop_path();
    results.push(configure_mcp_json(
        "Claude Desktop",
        &claude_path,
        binary_path,
    ));

    // 2. Cursor IDE
    let cursor_path = get_cursor_path();
    results.push(configure_mcp_json("Cursor IDE", &cursor_path, binary_path));

    // 3. Windsurf
    let windsurf_path = get_windsurf_path();
    results.push(configure_mcp_json("Windsurf", &windsurf_path, binary_path));

    // 4. Cline (VS Code)
    let cline_path = get_cline_path();
    results.push(configure_mcp_json(
        "Cline (VS Code)",
        &cline_path,
        binary_path,
    ));

    // 5. Roo Code (VS Code)
    let roo_path = get_roo_code_path();
    results.push(configure_mcp_json(
        "Roo Code (VS Code)",
        &roo_path,
        binary_path,
    ));

    // 6. Zed Editor
    let zed_path = get_zed_path();
    results.push(configure_zed_json(&zed_path, binary_path));

    // 7. Continue.dev
    let continue_path = get_continue_path();
    results.push(configure_mcp_json(
        "Continue.dev",
        &continue_path,
        binary_path,
    ));

    results
}

fn atomic_write_json(parent: &Path, target: &Path, content: &str) -> Result<(), String> {
    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|e| format!("Failed to create temporary file for atomic write: {}", e))?;
    temp.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write to temporary file: {}", e))?;
    temp.flush()
        .map_err(|e| format!("Failed to flush temporary file: {}", e))?;
    temp.persist(target)
        .map_err(|e| format!("Atomic rename failed: {}", e))?;
    Ok(())
}

fn safe_backup(config_path: &Path) -> Result<PathBuf, String> {
    let backup_path = config_path.with_extension("json.bak");
    fs::copy(config_path, &backup_path).map_err(|e| format!("Failed to create backup: {}", e))?;

    let original_bytes = fs::read(config_path)
        .map_err(|e| format!("Failed to read original for verification: {}", e))?;
    let backup_bytes = fs::read(&backup_path)
        .map_err(|e| format!("Failed to read backup for verification: {}", e))?;

    if original_bytes != backup_bytes {
        return Err("Backup byte-equality verification failed. Aborting write.".into());
    }

    Ok(backup_path)
}

fn configure_mcp_json(ide_name: &str, config_path: &Path, binary_path: &str) -> SetupResult {
    let parent = match config_path.parent() {
        Some(p) => p,
        None => {
            return SetupResult {
                name: ide_name.to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: "Invalid configuration directory".to_string(),
            }
        }
    };

    if !config_path.exists() && !parent.exists() {
        return SetupResult {
            name: ide_name.to_string(),
            path: config_path.to_path_buf(),
            success: true,
            message: "Not detected on this system (skipped)".to_string(),
        };
    }

    if let Err(e) = fs::create_dir_all(parent) {
        return SetupResult {
            name: ide_name.to_string(),
            path: config_path.to_path_buf(),
            success: false,
            message: format!("Failed to create parent directory: {}", e),
        };
    }

    let mut config: Value = if config_path.exists() {
        match fs::read_to_string(config_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(parsed) if parsed.is_object() => parsed,
                Ok(_) => {
                    return SetupResult {
                        name: ide_name.to_string(),
                        path: config_path.to_path_buf(),
                        success: false,
                        message: "Existing configuration is not a valid JSON object. Aborting to prevent corruption.".to_string(),
                    };
                }
                Err(e) => {
                    return SetupResult {
                        name: ide_name.to_string(),
                        path: config_path.to_path_buf(),
                        success: false,
                        message: format!(
                            "Refusing to overwrite unparseable configuration file: {}",
                            e
                        ),
                    };
                }
            },
            Err(e) => {
                return SetupResult {
                    name: ide_name.to_string(),
                    path: config_path.to_path_buf(),
                    success: false,
                    message: format!("Failed to read existing configuration: {}", e),
                };
            }
        }
    } else {
        json!({})
    };

    if config_path.exists() {
        if let Err(err_msg) = safe_backup(config_path) {
            return SetupResult {
                name: ide_name.to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: err_msg,
            };
        }
    }

    let mcp_servers = config
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    if let Some(servers_obj) = mcp_servers.as_object_mut() {
        servers_obj.insert(
            "intermcp".to_string(),
            json!({
                "command": binary_path,
                "args": ["serve"]
            }),
        );
    }

    match serde_json::to_string_pretty(&config) {
        Ok(serialized) => match atomic_write_json(parent, config_path, &serialized) {
            Ok(_) => SetupResult {
                name: ide_name.to_string(),
                path: config_path.to_path_buf(),
                success: true,
                message: "Successfully configured and merged InterMCP server".to_string(),
            },
            Err(e) => SetupResult {
                name: ide_name.to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: format!("Failed atomic write: {}", e),
            },
        },
        Err(e) => SetupResult {
            name: ide_name.to_string(),
            path: config_path.to_path_buf(),
            success: false,
            message: format!("Serialization error: {}", e),
        },
    }
}

fn configure_zed_json(config_path: &Path, binary_path: &str) -> SetupResult {
    let parent = match config_path.parent() {
        Some(p) => p,
        None => {
            return SetupResult {
                name: "Zed Editor".to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: "Invalid configuration directory".to_string(),
            }
        }
    };

    if !config_path.exists() && !parent.exists() {
        return SetupResult {
            name: "Zed Editor".to_string(),
            path: config_path.to_path_buf(),
            success: true,
            message: "Not detected on this system (skipped)".to_string(),
        };
    }

    if let Err(e) = fs::create_dir_all(parent) {
        return SetupResult {
            name: "Zed Editor".to_string(),
            path: config_path.to_path_buf(),
            success: false,
            message: format!("Failed to create parent directory: {}", e),
        };
    }

    let mut config: Value = if config_path.exists() {
        match fs::read_to_string(config_path) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(parsed) if parsed.is_object() => parsed,
                Ok(_) => {
                    return SetupResult {
                        name: "Zed Editor".to_string(),
                        path: config_path.to_path_buf(),
                        success: false,
                        message: "Existing configuration is not a valid JSON object. Aborting to prevent corruption.".to_string(),
                    };
                }
                Err(e) => {
                    return SetupResult {
                        name: "Zed Editor".to_string(),
                        path: config_path.to_path_buf(),
                        success: false,
                        message: format!(
                            "Refusing to overwrite unparseable configuration file: {}",
                            e
                        ),
                    };
                }
            },
            Err(e) => {
                return SetupResult {
                    name: "Zed Editor".to_string(),
                    path: config_path.to_path_buf(),
                    success: false,
                    message: format!("Failed to read existing configuration: {}", e),
                };
            }
        }
    } else {
        json!({})
    };

    if config_path.exists() {
        if let Err(err_msg) = safe_backup(config_path) {
            return SetupResult {
                name: "Zed Editor".to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: err_msg,
            };
        }
    }

    let context_servers = config
        .as_object_mut()
        .unwrap()
        .entry("context_servers")
        .or_insert_with(|| json!({}));

    if let Some(servers_obj) = context_servers.as_object_mut() {
        servers_obj.insert(
            "intermcp".to_string(),
            json!({
                "command": {
                    "path": binary_path,
                    "args": ["serve"]
                }
            }),
        );
    }

    match serde_json::to_string_pretty(&config) {
        Ok(serialized) => match atomic_write_json(parent, config_path, &serialized) {
            Ok(_) => SetupResult {
                name: "Zed Editor".to_string(),
                path: config_path.to_path_buf(),
                success: true,
                message: "Successfully configured and merged InterMCP context server".to_string(),
            },
            Err(e) => SetupResult {
                name: "Zed Editor".to_string(),
                path: config_path.to_path_buf(),
                success: false,
                message: format!("Failed atomic write: {}", e),
            },
        },
        Err(e) => SetupResult {
            name: "Zed Editor".to_string(),
            path: config_path.to_path_buf(),
            success: false,
            message: format!("Serialization error: {}", e),
        },
    }
}

fn get_claude_desktop_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(appdata)
            .join("Claude")
            .join("claude_desktop_config.json")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude_desktop_config.json")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join(".config")
            .join("Claude")
            .join("claude_desktop_config.json")
    }
}

fn get_cursor_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let userprofile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(userprofile).join(".cursor").join("mcp.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home).join(".cursor").join("mcp.json")
    }
}

fn get_windsurf_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let userprofile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(userprofile)
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json")
    }
}

fn get_cline_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(appdata)
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("saoudrizwan.claude-dev")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("saoudrizwan.claude-dev")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join(".config")
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("saoudrizwan.claude-dev")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
}

fn get_roo_code_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(appdata)
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join(".config")
            .join("Code")
            .join("User")
            .join("globalStorage")
            .join("rooveterinaryinc.roo-cline")
            .join("settings")
            .join("cline_mcp_settings.json")
    }
}

fn get_zed_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(appdata).join("Zed").join("settings.json")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Zed")
            .join("settings.json")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home)
            .join(".config")
            .join("zed")
            .join("settings.json")
    }
}

fn get_continue_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let userprofile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
        PathBuf::from(userprofile)
            .join(".continue")
            .join("config.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home).join(".continue").join("config.json")
    }
}
