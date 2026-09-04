use intermcp::auto_config::{auto_configure_all_ides, configure_mcp_json, configure_zed_json};
use serde_json::{json, Value};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_auto_configure_all_ides_contains_target_environments() {
    let results = auto_configure_all_ides("/mock/bin/intermcp");
    let names: Vec<String> = results.into_iter().map(|r| r.name).collect();

    assert!(names.contains(&"Antigravity IDE".to_string()));
    assert!(names.contains(&"Kilo Code (VS Code)".to_string()));
    assert!(names.contains(&"VS Code / Codex".to_string()));
    assert!(names.contains(&"Claude Desktop".to_string()));
    assert!(names.contains(&"Cursor IDE".to_string()));
    assert!(names.contains(&"Windsurf".to_string()));
    assert!(names.contains(&"Cline (VS Code)".to_string()));
    assert!(names.contains(&"Roo Code (VS Code)".to_string()));
    assert!(names.contains(&"Zed Editor".to_string()));
    assert!(names.contains(&"Continue.dev".to_string()));
}

#[test]
fn test_configure_mcp_json_creates_new_config() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("mcp_config.json");

    let result = configure_mcp_json("Antigravity IDE", &config_path, "/path/to/intermcp");
    assert!(result.success);

    let content = fs::read_to_string(&config_path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();

    assert_eq!(
        parsed["mcpServers"]["intermcp"]["command"],
        "/path/to/intermcp"
    );
    assert_eq!(parsed["mcpServers"]["intermcp"]["args"], json!(["serve"]));
}

#[test]
fn test_configure_mcp_json_merges_without_clobbering_and_creates_backup() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("mcp_settings.json");

    let initial = json!({
        "mcpServers": {
            "pre-existing-tool": {
                "command": "node",
                "args": ["index.js"]
            }
        },
        "userPreferences": {
            "telemetry": false
        }
    });

    fs::write(
        &config_path,
        serde_json::to_string_pretty(&initial).unwrap(),
    )
    .unwrap();

    let result = configure_mcp_json(
        "Kilo Code (VS Code)",
        &config_path,
        "/opt/intermcp/intermcp",
    );
    assert!(result.success);

    // Verify backup exists
    let backup_path = config_path.with_extension("json.bak");
    assert!(backup_path.exists());
    let backup_content = fs::read_to_string(&backup_path).unwrap();
    let backup_parsed: Value = serde_json::from_str(&backup_content).unwrap();
    assert_eq!(backup_parsed["userPreferences"]["telemetry"], false);
    assert!(backup_parsed["mcpServers"].get("intermcp").is_none());

    // Verify merged config
    let updated_content = fs::read_to_string(&config_path).unwrap();
    let updated_parsed: Value = serde_json::from_str(&updated_content).unwrap();
    assert_eq!(updated_parsed["userPreferences"]["telemetry"], false);
    assert_eq!(
        updated_parsed["mcpServers"]["pre-existing-tool"]["command"],
        "node"
    );
    assert_eq!(
        updated_parsed["mcpServers"]["intermcp"]["command"],
        "/opt/intermcp/intermcp"
    );
}

#[test]
fn test_configure_mcp_json_aborts_on_corrupt_json() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("corrupt_config.json");

    let corrupt_content = "{ invalid json content ...";
    fs::write(&config_path, corrupt_content).unwrap();

    let result = configure_mcp_json("Antigravity IDE", &config_path, "/path/to/intermcp");
    assert!(!result.success);
    assert!(result
        .message
        .contains("Refusing to overwrite unparseable configuration file"));

    // Ensure corrupt file was NOT overwritten or destroyed
    let after = fs::read_to_string(&config_path).unwrap();
    assert_eq!(after, corrupt_content);
}

#[test]
fn test_configure_zed_json_format() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("settings.json");

    let result = configure_zed_json(&config_path, "/path/to/intermcp");
    assert!(result.success);

    let content = fs::read_to_string(&config_path).unwrap();
    let parsed: Value = serde_json::from_str(&content).unwrap();

    assert_eq!(
        parsed["context_servers"]["intermcp"]["command"]["path"],
        "/path/to/intermcp"
    );
    assert_eq!(
        parsed["context_servers"]["intermcp"]["command"]["args"],
        json!(["serve"])
    );
}
