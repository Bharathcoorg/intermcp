use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};
use serde_json::{json, Value};

pub const GRAVITY_IS_MOCK: bool = true;

const DEMO_WARNING_HEADER: &str =
    "⚠️  DEMO PLACEHOLDER — values are hardcoded, do NOT use for trading decisions.";

pub fn create_gravity_market_price_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_get_market_price",
        "[DEMO ONLY] Query live market prices on Gravity DEX (Universal Omni-VM Superchain Trading Terminal)",
        json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "string",
                    "description": "Trading pair symbol, e.g., 'GRAV/USDC', 'ETH/USDC', 'SOL/USDC', 'DOT/USDC'"
                }
            },
            "required": ["pair"]
        }),
        |args: Value| async move {
            if !GRAVITY_IS_MOCK {
                return Err(crate::error::FastMcpError::ToolExecution(
                    "Real RPC execution is not implemented in this build".into(),
                ));
            }

            let pair = args
                .get("pair")
                .and_then(|v| v.as_str())
                .unwrap_or("GRAV/USDC")
                .to_uppercase();

            let (price, change_24h, volume_24h) = match pair.as_str() {
                "GRAV/USDC" => (4.85, "+14.2%", "$1,840,290"),
                "ETH/USDC" => (3420.50, "+2.8%", "$18,400,000"),
                "SOL/USDC" => (188.75, "+5.1%", "$9,210,000"),
                "DOT/USDC" => (8.42, "-0.8%", "$840,000"),
                _ => (1.00, "0.0%", "$100,000"),
            };

            let data = json!({
                "demoWarning": "⚠️ DEMO PLACEHOLDER — values are hardcoded. Do NOT use for trading decisions.",
                "network": "Gravity Omni-VM Testnet",
                "dex": "Gravity DEX Superchain Terminal",
                "pair": pair,
                "priceUsd": price,
                "change24h": change_24h,
                "volume24h": volume_24h,
                "verified": true,
                "executionEngine": "PolkaVM / RISC-V + Wasm Hybrid"
            });

            let json_str = serde_json::to_string_pretty(&data).unwrap_or_default();
            Ok(CallToolResult::text(format!(
                "{}\n{}",
                DEMO_WARNING_HEADER, json_str
            )))
        },
    ))
}

pub fn create_gravity_pools_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_get_liquidity_pools",
        "[DEMO ONLY] List all active liquidity pools and TVL on Gravity DEX across Omni-VM engines (Wasm, RISC-V, EVM)",
        json!({
            "type": "object",
            "properties": {
                "filter_vm": {
                    "type": "string",
                    "description": "Optional VM filter: 'wasm', 'riscv', 'evm', or 'all'",
                    "enum": ["wasm", "riscv", "evm", "all"]
                }
            }
        }),
        |args: Value| async move {
            if !GRAVITY_IS_MOCK {
                return Err(crate::error::FastMcpError::ToolExecution(
                    "Real RPC execution is not implemented in this build".into(),
                ));
            }

            let filter = args
                .get("filter_vm")
                .and_then(|v| v.as_str())
                .unwrap_or("all")
                .to_lowercase();

            let pools = vec![
                json!({
                    "poolId": "grav-usdc-01",
                    "pair": "GRAV/USDC",
                    "vmType": "riscv",
                    "engine": "PolkaVM Bare-Metal RISC-V",
                    "tvlUsd": 4_250_000,
                    "feeTier": "0.05%",
                    "apr": "24.5%"
                }),
                json!({
                    "poolId": "eth-usdc-01",
                    "pair": "ETH/USDC",
                    "vmType": "evm",
                    "engine": "EVM Parallel Execution",
                    "tvlUsd": 12_800_000,
                    "feeTier": "0.30%",
                    "apr": "11.2%"
                }),
                json!({
                    "poolId": "sol-grav-01",
                    "pair": "SOL/GRAV",
                    "vmType": "wasm",
                    "engine": "Wasm Core VM",
                    "tvlUsd": 2_100_000,
                    "feeTier": "0.25%",
                    "apr": "18.9%"
                }),
            ];

            let filtered_pools: Vec<_> = if filter == "all" {
                pools
            } else {
                pools.into_iter().filter(|p| p["vmType"].as_str() == Some(&filter)).collect()
            };

            let response = json!({
                "demoWarning": "⚠️ DEMO PLACEHOLDER — values are hardcoded. Do NOT use for trading decisions.",
                "dex": "Gravity DEX",
                "totalPools": filtered_pools.len(),
                "pools": filtered_pools,
                "status": "online",
                "blockHeight": 1_842_901
            });

            let json_str = serde_json::to_string_pretty(&response).unwrap_or_default();
            Ok(CallToolResult::text(format!(
                "{}\n{}",
                DEMO_WARNING_HEADER, json_str
            )))
        },
    ))
}

pub fn create_gravity_simulate_swap_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_simulate_swap",
        "[DEMO ONLY] Simulate an Omni-VM cross-chain swap on Gravity DEX calculating exact execution output, routing, and price impact",
        json!({
            "type": "object",
            "properties": {
                "from_token": { "type": "string", "description": "Token to sell, e.g. 'ETH'" },
                "to_token": { "type": "string", "description": "Token to buy, e.g. 'GRAV'" },
                "amount_in": { "type": "number", "description": "Amount of from_token to swap" }
            },
            "required": ["from_token", "to_token", "amount_in"]
        }),
        |args: Value| async move {
            if !GRAVITY_IS_MOCK {
                return Err(crate::error::FastMcpError::ToolExecution(
                    "Real RPC execution is not implemented in this build".into(),
                ));
            }

            let from = args.get("from_token").and_then(|v| v.as_str()).unwrap_or("ETH").to_uppercase();
            let to = args.get("to_token").and_then(|v| v.as_str()).unwrap_or("GRAV").to_uppercase();
            let raw_amount = args.get("amount_in").and_then(|v| v.as_f64()).unwrap_or(1.0);
            if raw_amount.is_nan() || raw_amount.is_infinite() {
                return Ok(CallToolResult::error("amount_in must be a valid finite number"));
            }
            let amount_in = raw_amount.clamp(-1e12, 1e12);

            // Constant-product (x * y = k) AMM reserve configuration
            let (reserve_in, reserve_out, token_in_usd_price) = match (from.as_str(), to.as_str()) {
                ("ETH", "GRAV") => (5_000.0, 3_526_250.0, 3400.0),
                ("GRAV", "ETH") => (3_526_250.0, 5_000.0, 4.85),
                ("USDC", "GRAV") => (10_000_000.0, 2_061_855.0, 1.0),
                ("GRAV", "USDC") => (2_061_855.0, 10_000_000.0, 4.85),
                _ => (1_000_000.0, 1_000_000.0, 1.0),
            };

            // Uniswap v2 constant-product formula with 0.3% LP fee deduction
            let fee_multiplier = 0.997; // 30 bps fee
            let amount_in_effective = amount_in * fee_multiplier;
            if reserve_in + amount_in_effective <= 0.0 {
                return Ok(CallToolResult::error("Invalid reserve calculation resulted in zero or negative denominator"));
            }
            let estimated_out = (reserve_out * amount_in_effective) / (reserve_in + amount_in_effective);

            let spot_price = reserve_out / reserve_in;
            let execution_price = estimated_out / amount_in.max(f64::EPSILON);
            let price_impact_pct = ((spot_price - execution_price) / spot_price).max(0.0) * 100.0;
            let fee_usd = (amount_in * 0.003 * token_in_usd_price).min(500.0);

            let result = json!({
                "demoWarning": "⚠️ DEMO PLACEHOLDER — values are hardcoded. Do NOT use for trading decisions.",
                "dex": "Gravity DEX Omni-VM",
                "swapRoute": format!("{} -> Gravity Superchain Router -> {}", from, to),
                "amountIn": amount_in,
                "tokenIn": from,
                "estimatedOut": (estimated_out * 10_000.0).round() / 10_000.0,
                "tokenOut": to,
                "spotPrice": (spot_price * 10_000.0).round() / 10_000.0,
                "executionPrice": (execution_price * 10_000.0).round() / 10_000.0,
                "feeUsd": (fee_usd * 100.0).round() / 100.0,
                "priceImpact": format!("{:.2}%", price_impact_pct),
                "executionEngine": "PolkaVM RISC-V Instant Settlement",
                "gasUsedCycles": 42_010,
                "readyToBroadcast": true
            });

            let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
            Ok(CallToolResult::text(format!(
                "{}\n{}",
                DEMO_WARNING_HEADER, json_str
            )))
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gravity_tools_contain_demo_warning() {
        let price_tool = create_gravity_market_price_tool();
        let pools_tool = create_gravity_pools_tool();
        let swap_tool = create_gravity_simulate_swap_tool();

        let res_price = price_tool
            .execute(json!({"pair": "GRAV/USDC"}))
            .await
            .unwrap();
        let text_price = match &res_price.content[0] {
            crate::protocol::ContentItem::Text { text } => text.clone(),
            _ => panic!("Expected text"),
        };
        assert!(text_price.contains("demoWarning"));

        let res_pools = pools_tool.execute(json!({})).await.unwrap();
        let text_pools = match &res_pools.content[0] {
            crate::protocol::ContentItem::Text { text } => text.clone(),
            _ => panic!("Expected text"),
        };
        assert!(text_pools.contains("demoWarning"));

        let res_swap = swap_tool
            .execute(json!({"from_token": "ETH", "to_token": "GRAV", "amount_in": 1.0}))
            .await
            .unwrap();
        let text_swap = match &res_swap.content[0] {
            crate::protocol::ContentItem::Text { text } => text.clone(),
            _ => panic!("Expected text"),
        };
        assert!(text_swap.contains("demoWarning"));

        // Test division by zero / negative reserve guard
        let res_err = swap_tool
            .execute(json!({"from_token": "ETH", "to_token": "GRAV", "amount_in": -10_000.0}))
            .await
            .unwrap();
        assert!(res_err.is_error);
    }
}
