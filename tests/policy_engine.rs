use intermcp::policy::{PolicyEngine, PolicyMode, ShellPolicyDecision};
use std::path::Path;

#[test]
fn test_declarative_policy_from_toml() {
    let toml_str = r#"
        mode = "enforcing"

        [filesystem]
        read_only = ["./docs", "./config"]
        read_write = ["./src", "./target"]
        denied = [".env", "id_rsa", ".git"]

        [shell]
        allowed_binaries = ["git", "cargo", "node"]
        blocked_patterns = ["rm -rf", "mkfs"]
        require_approval = ["git push", "cargo publish"]

        [limits]
        max_calls_per_minute = 10
        max_output_bytes = 1048576
    "#;

    let engine = PolicyEngine::from_toml(toml_str).unwrap();
    assert_eq!(engine.mode(), PolicyMode::Enforcing);

    // Test filesystem read
    assert!(engine
        .check_filesystem(Path::new("./docs/readme.md"), false)
        .is_ok());
    assert!(engine
        .check_filesystem(Path::new("./src/main.rs"), false)
        .is_ok());

    // Test filesystem write
    assert!(engine
        .check_filesystem(Path::new("./src/lib.rs"), true)
        .is_ok());
    assert!(engine
        .check_filesystem(Path::new("./docs/readme.md"), true)
        .is_err());

    // Test denied patterns
    assert!(engine
        .check_filesystem(Path::new("./src/.env.local"), false)
        .is_err());
    assert!(engine
        .check_filesystem(Path::new("~/.ssh/id_rsa"), false)
        .is_err());
    // SEC-06 regression: Normalization prevents dot-segment bypass of denied rules
    assert!(engine
        .check_filesystem(Path::new("./src/./.env.local"), false)
        .is_err());
    assert!(engine
        .check_filesystem(Path::new("./src/nested/../.env.local"), false)
        .is_err());

    // Test shell binary allowlist
    let git_decision = engine.check_shell("git", "git status").unwrap();
    assert_eq!(git_decision, ShellPolicyDecision::Allow);

    assert!(engine
        .check_shell("curl", "curl https://example.com")
        .is_err());

    // Test blocked command patterns
    assert!(engine.check_shell("git", "git status && rm -rf /").is_err());

    // Test supervisor approval requirement
    let push_decision = engine.check_shell("git", "git push origin main").unwrap();
    assert!(matches!(
        push_decision,
        ShellPolicyDecision::RequireSupervisorApproval(_)
    ));

    // Test rate limiter
    for _ in 0..10 {
        assert!(engine.check_rate_limit("fs_read_file").is_ok());
    }
    // 11th call exceeds limit of 10
    assert!(engine.check_rate_limit("fs_read_file").is_err());
    assert!(engine.total_violations() > 0);
}
