use intermcp::protocol::JsonRpcResponse;
use intermcp::record::{SessionRecorder, SessionReplayer};
use intermcp::Server;
use serde_json::json;

#[tokio::test]
async fn test_session_recorder_and_replay() {
    let temp = tempfile::tempdir().unwrap();
    let trace_path = temp.path().join("trace.jsonl");

    let recorder = SessionRecorder::new(&trace_path).unwrap();
    let server = Server::new("test-recorder", "0.1.0").with_recorder(recorder);

    let ping_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "ping"
    })
    .to_string();

    let resp_str = server.handle_raw_message(&ping_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert!(resp.result.is_some());

    let replay_server = Server::new("test-recorder-replay", "0.1.0");
    let summary = SessionReplayer::replay(&trace_path, &replay_server)
        .await
        .unwrap();

    assert_eq!(summary.total_calls, 1);
    assert_eq!(summary.matched, 1);
    assert_eq!(summary.mismatched, 0);
}
