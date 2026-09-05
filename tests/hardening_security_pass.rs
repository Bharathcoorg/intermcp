use intermcp::protocol::JsonRpcResponse;
use intermcp::record::{SessionRecorder, SessionReplayer};
use intermcp::sandbox::SandboxPolicy;
use intermcp::smac::SmacLogger;
use intermcp::tools::fs::create_fs_search_tool;
use intermcp::tools::system::validate_shell_command;
use intermcp::Server;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_shell_linter_blocks_path_qualified_binaries_and_env_prefixes() {
    let allowed = ["git".to_string(), "cargo".to_string()];

    // Relative path qualification attempts
    assert!(validate_shell_command("./git status", &allowed).is_err());
    assert!(validate_shell_command("../git status", &allowed).is_err());

    // Absolute Unix / Windows path attempts
    assert!(validate_shell_command("/bin/git status", &allowed).is_err());
    assert!(validate_shell_command("/usr/bin/git status", &allowed).is_err());
    assert!(validate_shell_command("C:\\Windows\\git.exe status", &allowed).is_err());

    // Inline environment variable assignment attempt
    assert!(validate_shell_command("FOO=bar git status", &allowed).is_err());
    assert!(validate_shell_command("VAR1=val1 VAR2=val2 git status", &allowed).is_err());

    // Legitimate invocation succeeds
    assert!(validate_shell_command("git status", &allowed).is_ok());
    assert!(validate_shell_command("cargo check --all-targets", &allowed).is_ok());
}

#[tokio::test]
async fn test_chained_commands_verified_against_policy() {
    use intermcp::policy::{DeclarativePolicy, PolicyEngine, PolicyMode, ShellPolicy};

    let engine = PolicyEngine::new(DeclarativePolicy {
        mode: PolicyMode::Enforcing,
        shell: ShellPolicy {
            allowed_binaries: vec!["git".to_string()],
            blocked_patterns: vec!["evil_pattern".to_string()],
            require_approval: vec![],
        },
        ..Default::default()
    });

    let server = Server::new("test-chained-policy", "0.1.0").with_policy_engine(engine);

    // Chained command where first command is allowed (git) but second is not (whoami)
    let bad_chained_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "system_run_command",
            "arguments": {
                "command": "git status && whoami"
            }
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&bad_chained_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    let result = resp.result.unwrap();
    let is_error = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        is_error,
        "Expected policy violation error for disallowed subcommand 'whoami'"
    );
}

#[test]
fn test_smac_hash_value_rfc8785_canonicalization() {
    // Two JSON values with identical keys/values but different structural ordering
    let v1 = json!({
        "zebra": 100,
        "apple": "fruit",
        "nested": {
            "z": true,
            "a": false
        }
    });

    let v2 = json!({
        "apple": "fruit",
        "nested": {
            "a": false,
            "z": true
        },
        "zebra": 100
    });

    let h1 = SmacLogger::hash_value(&v1);
    let h2 = SmacLogger::hash_value(&v2);

    assert_eq!(
        h1, h2,
        "SmacLogger::hash_value must be deterministic and RFC 8785 canonical"
    );
}

#[tokio::test]
async fn test_session_replayer_blocks_mutations_by_default() {
    let temp = NamedTempFile::new().unwrap();
    let trace_path = temp.path().to_path_buf();

    let recorder = SessionRecorder::new(&trace_path).unwrap();
    let server = Server::new("test-recorder-mutating", "0.1.0").with_recorder(recorder);

    // Record a mutating request
    let mutate_req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "system_run_command",
            "arguments": {
                "command": "echo malicious"
            }
        }
    })
    .to_string();

    let _ = server.handle_raw_message(&mutate_req).await;

    // Now replay without --allow-mutations (default)
    let replay_server = Server::new("test-replay-target", "0.1.0");
    let summary = SessionReplayer::replay(&trace_path, &replay_server)
        .await
        .unwrap();

    assert_eq!(summary.total_calls, 1);
    assert_eq!(summary.mismatched, 1);
    assert!(!summary.errors.is_empty());
    assert!(summary.errors[0].contains("Safety guard"));
}

#[tokio::test]
async fn test_fs_search_text_respects_custom_sensitive_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // Create a normal file
    let normal_file = root.join("normal.txt");
    let mut f1 = File::create(&normal_file).unwrap();
    writeln!(f1, "target_keyword_in_normal").unwrap();

    // Create a custom sensitive file
    let sensitive_file = root.join("company_vault.secrets");
    let mut f2 = File::create(&sensitive_file).unwrap();
    writeln!(f2, "target_keyword_in_secrets").unwrap();

    // Configure sandbox policy with custom sensitive pattern
    let sb = SandboxPolicy::new(vec![root.to_path_buf()])
        .with_additional_sensitive_files(vec!["company_vault.secrets".to_string()]);

    let tool = create_fs_search_tool(sb);
    let args = json!({
        "query": "target_keyword",
        "dir": root.to_str().unwrap()
    });

    let res = tool.execute(args).await.unwrap();
    let content = match &res.content[0] {
        intermcp::protocol::ContentItem::Text { text } => text,
        _ => panic!("Expected text content"),
    };

    assert!(
        content.contains("normal.txt"),
        "Should find match in normal file"
    );
    assert!(
        !content.contains("company_vault.secrets"),
        "Must NOT search inside sensitive file"
    );
}
