use intermcp::protocol::ContentItem;
use intermcp::tools::gravity::{
    create_gravity_market_price_tool, create_gravity_pools_tool, create_gravity_simulate_swap_tool,
};
use intermcp::tools::plugin_toolset;
use serde_json::{json, Value};

#[tokio::test]
async fn test_gravity_plugin_toolset_loading() {
    let tools = plugin_toolset("gravity");
    assert_eq!(tools.len(), 3);
    assert_eq!(tools[0].name(), "gravity_get_market_price");
    assert_eq!(tools[1].name(), "gravity_get_liquidity_pools");
    assert_eq!(tools[2].name(), "gravity_simulate_swap");

    let interlayer_tools = plugin_toolset("interlayer");
    assert_eq!(interlayer_tools.len(), 3);

    let unknown_tools = plugin_toolset("unknown_plugin");
    assert!(unknown_tools.is_empty());
}

#[tokio::test]
async fn test_gravity_market_price_tool() {
    let tool = create_gravity_market_price_tool();
    assert_eq!(tool.name(), "gravity_get_market_price");

    // Test GRAV/USDC
    let res = tool
        .execute(json!({ "pair": "GRAV/USDC" }))
        .await
        .expect("Tool execution failed");
    assert!(!res.is_error);

    if let ContentItem::Text { text } = &res.content[0] {
        let parsed: Value = serde_json::from_str(text).expect("Valid JSON");
        assert_eq!(parsed["pair"], "GRAV/USDC");
        assert_eq!(parsed["priceUsd"], 4.85);
        assert_eq!(parsed["dex"], "Gravity DEX Superchain Terminal");
    } else {
        panic!("Expected text output");
    }

    // Test ETH/USDC
    let res_eth = tool
        .execute(json!({ "pair": "ETH/USDC" }))
        .await
        .expect("ETH price failed");
    if let ContentItem::Text { text } = &res_eth.content[0] {
        let parsed: Value = serde_json::from_str(text).expect("Valid JSON");
        assert_eq!(parsed["priceUsd"], 3420.50);
    }
}

#[tokio::test]
async fn test_gravity_pools_tool_filtering() {
    let tool = create_gravity_pools_tool();
    assert_eq!(tool.name(), "gravity_get_liquidity_pools");

    // Test filter "all"
    let res_all = tool.execute(json!({ "filter_vm": "all" })).await.unwrap();
    if let ContentItem::Text { text } = &res_all.content[0] {
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["totalPools"], 3);
    }

    // Test filter "wasm"
    let res_wasm = tool.execute(json!({ "filter_vm": "wasm" })).await.unwrap();
    if let ContentItem::Text { text } = &res_wasm.content[0] {
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["totalPools"], 1);
        assert_eq!(parsed["pools"][0]["vmType"], "wasm");
    }

    // Test filter "riscv"
    let res_riscv = tool.execute(json!({ "filter_vm": "riscv" })).await.unwrap();
    if let ContentItem::Text { text } = &res_riscv.content[0] {
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["totalPools"], 1);
        assert_eq!(parsed["pools"][0]["vmType"], "riscv");
    }
}

#[tokio::test]
async fn test_gravity_simulate_swap_tool() {
    let tool = create_gravity_simulate_swap_tool();
    assert_eq!(tool.name(), "gravity_simulate_swap");

    let res = tool
        .execute(json!({
            "from_token": "ETH",
            "to_token": "GRAV",
            "amount_in": 2.0
        }))
        .await
        .unwrap();

    assert!(!res.is_error);
    if let ContentItem::Text { text } = &res.content[0] {
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["tokenIn"], "ETH");
        assert_eq!(parsed["tokenOut"], "GRAV");
        assert_eq!(parsed["amountIn"], 2.0);
        assert!(parsed["estimatedOut"].as_f64().unwrap() > 1400.0);
        assert_eq!(parsed["readyToBroadcast"], true);
    } else {
        panic!("Expected text output");
    }
}
