use intermcp::hub::{HubConfig, UpstreamServerConfig};
use std::collections::HashMap;

#[test]
fn test_hub_config_serialization_and_defaults() {
    let mut env = HashMap::new();
    env.insert("DEBUG".to_string(), "1".to_string());

    let config = HubConfig {
        servers: vec![UpstreamServerConfig {
            name: "test-node".to_string(),
            command: "node".to_string(),
            args: vec!["server.js".to_string()],
            env,
        }],
    };

    let serialized = serde_json::to_string(&config).unwrap();
    let deserialized: HubConfig = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.servers.len(), 1);
    assert_eq!(deserialized.servers[0].name, "test-node");
    assert_eq!(deserialized.servers[0].command, "node");
    assert_eq!(deserialized.servers[0].args, vec!["server.js"]);
    assert_eq!(deserialized.servers[0].env.get("DEBUG").unwrap(), "1");
}
