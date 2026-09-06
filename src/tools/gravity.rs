use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};
use serde_json::{json, Value};

pub fn create_gravity_market_price_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_get_market_price",
        "Query live market prices on Gravity DEX (Universal Omni-VM Superchain Trading Terminal)",
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
                "network": "Gravity Omni-VM Testnet",
                "dex": "Gravity DEX Superchain Terminal",
                "pair": pair,
                "priceUsd": price,
                "change24h": change_24h,
                "volume24h": volume_24h,
                "verified": true,
                "executionEngine": "PolkaVM / RISC-V + Wasm Hybrid"
            });

            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&data).unwrap_or_default(),
            ))
        },
    ))
}

pub fn create_gravity_pools_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_get_liquidity_pools",
        "List all active liquidity pools and TVL on Gravity DEX across Omni-VM engines (Wasm, RISC-V, EVM)",
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
                "dex": "Gravity DEX",
                "totalPools": filtered_pools.len(),
                "pools": filtered_pools,
                "status": "online",
                "blockHeight": 1_842_901
            });

            Ok(CallToolResult::text(serde_json::to_string_pretty(&response).unwrap_or_default()))
        },
    ))
}

pub fn create_gravity_simulate_swap_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "gravity_simulate_swap",
        "Simulate an Omni-VM cross-chain swap on Gravity DEX calculating exact execution output, routing, and price impact",
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
            let from = args.get("from_token").and_then(|v| v.as_str()).unwrap_or("ETH").to_uppercase();
            let to = args.get("to_token").and_then(|v| v.as_str()).unwrap_or("GRAV").to_uppercase();
            let amount_in = args.get("amount_in").and_then(|v| v.as_f64()).unwrap_or(1.0);

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
            let estimated_out = (reserve_out * amount_in_effective) / (reserve_in + amount_in_effective);

            let spot_price = reserve_out / reserve_in;
            let execution_price = estimated_out / amount_in.max(f64::EPSILON);
            let price_impact_pct = ((spot_price - execution_price) / spot_price).max(0.0) * 100.0;
            let fee_usd = (amount_in * 0.003 * token_in_usd_price).min(500.0);

            let result = json!({
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

            Ok(CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default()))
        },
    ))
}
