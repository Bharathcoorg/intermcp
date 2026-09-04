use intermcp::taint::{SensitivityLabel, SinkCapability, TaintTracker};
use serde_json::json;

#[test]
fn test_taint_tracking_and_flow_enforcement() {
    let tracker = TaintTracker::new();

    // 1. Tagging items
    tracker.tag_item("user_prompt", SensitivityLabel::Public);
    tracker.tag_item("repo_code", SensitivityLabel::Internal);
    tracker.tag_item("aws_key", SensitivityLabel::Confidential);
    tracker.tag_item("web_search_snippet", SensitivityLabel::Untrusted);

    assert_eq!(tracker.get_label("user_prompt"), SensitivityLabel::Public);
    assert_eq!(
        tracker.get_label("web_search_snippet"),
        SensitivityLabel::Untrusted
    );

    // 2. Permitted flows
    assert!(tracker
        .check_flow(
            SensitivityLabel::Public,
            SinkCapability::PrivilegedExecution
        )
        .is_ok());
    assert!(tracker
        .check_flow(SensitivityLabel::Internal, SinkCapability::FileMutation)
        .is_ok());
    assert!(tracker
        .check_flow(
            SensitivityLabel::Untrusted,
            SinkCapability::ReadOnlyInspection
        )
        .is_ok());

    // 3. Prohibited flow: Untrusted data flowing to privileged shell execution
    let untrusted_flow = tracker.check_flow(
        SensitivityLabel::Untrusted,
        SinkCapability::PrivilegedExecution,
    );
    assert!(untrusted_flow.is_err());

    // 4. Prohibited flow: Confidential data egressing to external network
    let confidential_flow = tracker.check_flow(
        SensitivityLabel::Confidential,
        SinkCapability::NetworkEgress,
    );
    assert!(confidential_flow.is_err());

    // 5. JSON argument scanning
    let safe_args = json!({ "path": "src/main.rs" });
    assert!(tracker
        .scan_json_arguments(&safe_args, SinkCapability::PrivilegedExecution)
        .is_ok());

    let tainted_args = json!({
        "command": "deploy.sh",
        "_taint": "untrusted"
    });
    assert!(tracker
        .scan_json_arguments(&tainted_args, SinkCapability::PrivilegedExecution)
        .is_err());
    assert_eq!(tracker.total_violations(), 3);
}
