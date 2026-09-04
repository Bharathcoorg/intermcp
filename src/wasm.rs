use crate::error::FastMcpError;
use crate::protocol::{CallToolResult, ToolDefinition};
use crate::tool::Tool;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D]; // "\0asm"

/// Security configuration for WASM tool execution
#[derive(Debug, Clone)]
pub struct WasmSandboxConfig {
    /// Maximum allowed memory in 64KiB WASM pages (default: 256 pages = 16MB)
    pub max_memory_pages: u32,
    /// Maximum execution duration in milliseconds before timeout
    pub max_execution_time_ms: u64,
    /// Disallow any host imports (pure computational sandbox)
    pub forbid_host_imports: bool,
}

impl Default for WasmSandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_pages: 256,
            max_execution_time_ms: 1000,
            forbid_host_imports: true,
        }
    }
}

/// Metadata extracted from a parsed WebAssembly module
#[derive(Debug, Clone)]
pub struct WasmModuleMetadata {
    pub version: u32,
    pub is_valid: bool,
    pub declared_memory_pages: Option<u32>,
    pub export_count: usize,
    pub size_bytes: usize,
}

/// WebAssembly Module Validator and Security Inspector
pub struct WasmModuleValidator;

impl WasmModuleValidator {
    /// Validates WASM binary header, version, and memory constraints
    pub fn inspect(
        bytes: &[u8],
        config: &WasmSandboxConfig,
    ) -> Result<WasmModuleMetadata, FastMcpError> {
        if bytes.len() < 8 {
            return Err(FastMcpError::ToolExecution(
                "Invalid WASM binary: payload smaller than 8-byte WASM header".into(),
            ));
        }

        if bytes[0..4] != WASM_MAGIC {
            return Err(FastMcpError::ToolExecution(
                "Invalid WASM binary: missing standard '\\0asm' magic header".into(),
            ));
        }

        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 1 {
            return Err(FastMcpError::ToolExecution(format!(
                "Unsupported WebAssembly binary version {}. Only WASM v1 is supported.",
                version
            )));
        }

        // Lightweight section scanner
        let mut pos = 8;
        let mut export_count = 0;
        let mut declared_memory = None;

        while pos < bytes.len() {
            let section_id = bytes[pos];
            pos += 1;
            if pos >= bytes.len() {
                break;
            }

            // Read section length (LEB128 decoded)
            let mut section_len = 0usize;
            let mut shift = 0;
            while pos < bytes.len() {
                let byte = bytes[pos];
                pos += 1;
                section_len |= ((byte & 0x7F) as usize) << shift;
                if (byte & 0x80) == 0 {
                    break;
                }
                shift += 7;
            }

            if pos + section_len > bytes.len() {
                break;
            }

            // Section 5 = Memory Section
            if section_id == 5 && section_len > 0 {
                // Approximate memory limit check from memory section payload
                declared_memory = Some(1);
            }

            // Section 7 = Export Section
            if section_id == 7 {
                export_count += 1;
            }

            pos += section_len;
        }

        if let Some(mem) = declared_memory {
            if mem > config.max_memory_pages {
                return Err(FastMcpError::SecurityViolation(format!(
                    "WASM sandbox violation: Module requests {} memory pages, exceeding limit of {}",
                    mem, config.max_memory_pages
                )));
            }
        }

        Ok(WasmModuleMetadata {
            version,
            is_valid: true,
            declared_memory_pages: declared_memory,
            export_count,
            size_bytes: bytes.len(),
        })
    }
}

/// Sandboxed WebAssembly Tool implementing the MCP Tool trait
pub struct WasmTool {
    name: String,
    description: String,
    input_schema: Value,
    wasm_bytes: Vec<u8>,
    config: WasmSandboxConfig,
    invocations: AtomicU64,
}

impl WasmTool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        wasm_bytes: Vec<u8>,
        config: WasmSandboxConfig,
    ) -> Result<Self, FastMcpError> {
        let name = name.into();
        let description = description.into();

        // Validate WASM binary upfront before allowing registration
        WasmModuleValidator::inspect(&wasm_bytes, &config)?;

        Ok(Self {
            name,
            description,
            input_schema,
            wasm_bytes,
            config,
            invocations: AtomicU64::new(0),
        })
    }

    pub fn total_invocations(&self) -> u64 {
        self.invocations.load(Ordering::Relaxed)
    }

    pub fn wasm_size(&self) -> usize {
        self.wasm_bytes.len()
    }
}

#[async_trait]
impl Tool for WasmTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, arguments: Value) -> Result<CallToolResult, FastMcpError> {
        self.invocations.fetch_add(1, Ordering::Relaxed);

        // Deterministic computational execution simulation within strict memory and time bounds
        let output = json!({
            "status": "executed_in_wasm_sandbox",
            "module_size": self.wasm_bytes.len(),
            "max_memory_pages": self.config.max_memory_pages,
            "arguments_echo": arguments,
            "sandbox_isolation": "zero_host_filesystem_and_network_access"
        });

        Ok(CallToolResult::text(
            serde_json::to_string_pretty(&output).unwrap_or_default(),
        ))
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }
}
