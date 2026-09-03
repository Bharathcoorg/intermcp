pub mod fs;
pub mod git;
pub mod gravity;
pub mod system;

use crate::sandbox::SandboxPolicy;
use crate::tool::Tool;

/// Pure universal developer toolset with optional SafeFS sandboxing
pub fn universal_toolset(sandbox: Option<SandboxPolicy>) -> Vec<Box<dyn Tool>> {
    let sb = sandbox.unwrap_or_else(SandboxPolicy::unrestricted);
    vec![
        fs::create_fs_read_tool(sb.clone()),
        fs::create_fs_write_tool(sb.clone()),
        fs::create_fs_list_tool(sb.clone()),
        fs::create_fs_search_tool(sb),
        git::create_git_status_tool(),
        git::create_git_diff_tool(),
        system::create_system_info_tool(),
        system::create_shell_exec_tool(),
    ]
}

/// Optional plugin toolsets (e.g. Gravity Omni-VM / DEX, Postgres, Docker)
pub fn plugin_toolset(plugin: &str) -> Vec<Box<dyn Tool>> {
    match plugin.to_lowercase().as_str() {
        "gravity" | "interlayer" => vec![
            gravity::create_gravity_market_price_tool(),
            gravity::create_gravity_pools_tool(),
            gravity::create_gravity_simulate_swap_tool(),
        ],
        _ => Vec::new(),
    }
}

/// Default toolset enabled out of the box (100% universal general-purpose developer tools)
pub fn default_toolset() -> Vec<Box<dyn Tool>> {
    universal_toolset(None)
}
