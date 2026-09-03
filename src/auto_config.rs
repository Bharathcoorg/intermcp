use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct SetupResult {
    pub name: String,
    pub path: PathBuf,
    pub success: bool,
    pub message: String,
}

pub fn auto_configure_all_ides(binary_path: &str) -> Vec<SetupResult> {
    let mut results = Vec::new();

    // 1. Claude Desktop
    let claude_path = get_claude_path();
    results.push(configure_mcp_json(
        "Claude Desktop",
        &claude_path,
        binary_path,
    ));

    // 2. Cursor IDE
    let cursor_path = get_cursor_path();
    results.push(configure_mcp_json("Cursor IDE", &cursor_path, binary_path));

    // 3. Windsurf Editor
    let windsurf_path = get_windsurf_path();
    results.push(configure_mcp_json(
        "Windsurf Editor",
        &windsurf_path,
        binary_path,
    ));

    // 4. Cline (VS Code Extension)
    let cline_path = get_cline_path();
    results.push(configure_mcp_json(
        "Cline (VS Code)",
        &cline_path,
        binary_path,
    ));

    results
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

    // If the editor app directory is not present on this machine, skip without creating ghost folders
    if !config_path.exists() && !parent.exists() {
        return SetupResult {
            name: ide_name.to_string(),
            path: config_path.to_path_buf(),
            success: true,
            message: "Not detected on this system (skipped)".to_string(),
        };
    }

    let _ = fs::create_dir_all(parent);

    let mut config: Value = if config_path.exists() {
        match fs::read_to_string(config_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| json!({})),
            Err(_) => json!({}),
        }
    } else {
        json!({})
    };

    if !config.is_object() {
        config = json!({});
    }

    // Backup existing configuration if file exists
    if config_path.exists() {
        let backup_path = config_path.with_extension("json.bak");
        let _ = fs::copy(config_path, backup_path);
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
        Ok(serialized) => match fs::write(config_path, serialized) {
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
                message: format!("Failed to write configuration: {}", e),
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

fn get_claude_path() -> PathBuf {
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
