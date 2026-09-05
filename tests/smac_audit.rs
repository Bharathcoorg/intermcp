use intermcp::smac::{verify_smac_log, SmacLogger};
use serde_json::json;
use std::fs::{read_to_string, write};

#[test]
fn test_smac_audit_chain_verification() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("audit.log");

    let logger = SmacLogger::new(&log_path).unwrap();

    logger.record(
        "fs_read_file",
        &json!({"path": "src/main.rs"}),
        &json!({"bytes": 1024}),
    );
    logger.record("git_status", &json!({}), &json!({"clean": true}));
    logger.record(
        "system_run_command",
        &json!({"command": "cargo check"}),
        &json!({"exitCode": 0}),
    );

    let verified = verify_smac_log(&log_path);
    assert!(verified.is_ok());
    assert_eq!(verified.unwrap(), 3);
}

#[test]
fn test_smac_tamper_detection() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("audit.log");

    let logger = SmacLogger::new(&log_path).unwrap();
    logger.record("tool_a", &json!({"a": 1}), &json!({"res": "ok"}));
    logger.record("tool_b", &json!({"b": 2}), &json!({"res": "ok"}));

    let original = read_to_string(&log_path).unwrap();
    let tampered = original.replace("\"tool_b\"", "\"tool_malicious\"");
    write(&log_path, tampered).unwrap();

    let verified = verify_smac_log(&log_path);
    assert!(verified.is_err());
    let err_msg = verified.unwrap_err();
    assert!(err_msg.contains("Tampering detected") || err_msg.contains("Chain broken"));
}
