use intermcp::manifest::load_manifest_tools;
use serde_json::json;
use std::path::PathBuf;

#[tokio::test]
async fn test_load_and_execute_example_tools_json() {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("example-tools.json");
    assert!(
        manifest_path.exists(),
        "example-tools.json must exist in repo root"
    );

    let tools = load_manifest_tools(&manifest_path).expect("Failed to parse example-tools.json");
    assert_eq!(tools.len(), 1);

    let tool = &tools[0];
    assert_eq!(tool.name(), "echo_custom");
    assert_eq!(
        tool.description(),
        "Echo back a custom message with a timestamp"
    );

    let result = tool
        .execute(json!({}))
        .await
        .expect("Tool execution failed");
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn test_declarative_tool_argument_interpolation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let manifest_file = temp_dir.path().join("tools.json");

    let manifest_json = json!({
        "tools": [
            {
                "name": "greet_user",
                "description": "Greets a user with custom message",
                "command": "git",
                "args": ["version"],
                "params": {
                    "username": {
                        "type": "string",
                        "description": "Name of user",
                        "required": true
                    }
                }
            }
        ]
    });

    std::fs::write(
        &manifest_file,
        serde_json::to_string(&manifest_json).unwrap(),
    )
    .unwrap();

    let tools = load_manifest_tools(&manifest_file).expect("Failed to load custom manifest");
    assert_eq!(tools.len(), 1);

    let tool = &tools[0];
    let res = tool
        .execute(json!({ "username": "developer" }))
        .await
        .expect("Execution succeeded");
    assert!(!res.is_error);
    if let intermcp::protocol::ContentItem::Text { text } = &res.content[0] {
        assert!(text.contains("git version"), "Expected git version output");
    } else {
        panic!("Expected text output");
    }
}

#[tokio::test]
async fn test_load_manifest_rejects_empty_command() {
    let temp_dir = tempfile::tempdir().unwrap();
    let manifest_file = temp_dir.path().join("invalid_tools.json");

    let manifest_json = json!({
        "tools": [
            {
                "name": "empty_cmd_tool",
                "description": "Tool with empty command",
                "command": "",
                "args": []
            }
        ]
    });

    std::fs::write(
        &manifest_file,
        serde_json::to_string(&manifest_json).unwrap(),
    )
    .unwrap();

    let res = load_manifest_tools(&manifest_file);
    assert!(res.is_err(), "Must reject manifest with empty command");
}

#[tokio::test]
async fn test_manifest_tool_output_truncation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let manifest_file = temp_dir.path().join("large_output_tool.json");

    #[cfg(windows)]
    let (cmd, args) = (
        "python",
        vec![
            "-c".to_string(),
            "import sys; sys.stdout.write('A' * 300000)".to_string(),
        ],
    );
    #[cfg(not(windows))]
    let (cmd, args) = ("seq", vec!["1".to_string(), "100000".to_string()]);

    let manifest_json = json!({
        "tools": [
            {
                "name": "large_output",
                "description": "Produces large output to test truncation",
                "command": cmd,
                "args": args
            }
        ]
    });

    std::fs::write(
        &manifest_file,
        serde_json::to_string(&manifest_json).unwrap(),
    )
    .unwrap();

    let tools = load_manifest_tools(&manifest_file).expect("Failed to load manifest");
    assert_eq!(tools.len(), 1);

    let res = tools[0]
        .execute(json!({}))
        .await
        .expect("Execution succeeded");
    assert!(!res.is_error);
    if let intermcp::protocol::ContentItem::Text { text } = &res.content[0] {
        assert!(
            text.contains("... [Output truncated: exceeded 256KB]"),
            "Output must contain truncation marker"
        );
    } else {
        panic!("Expected text output");
    }
}
