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
                if shift > 28 {
                    return Err(FastMcpError::ToolExecution(
                        "Invalid WASM binary: LEB128 integer overflow".into(),
                    ));
                }
                section_len |= ((byte & 0x7F) as usize) << shift;
                if (byte & 0x80) == 0 {
                    break;
                }
                shift += 7;
            }

            if pos + section_len > bytes.len() {
                return Err(FastMcpError::ToolExecution(
                    "Invalid WASM binary: section length exceeds payload".into(),
                ));
            }

            // Section 5 = Memory Section
            if section_id == 5 && section_len > 0 {
                let mut mem_pos = pos;
                let mem_end = pos + section_len;

                // Read vector count of memory types (LEB128)
                let mut count = 0usize;
                let mut shift = 0;
                while mem_pos < mem_end {
                    let b = bytes[mem_pos];
                    mem_pos += 1;
                    count |= ((b & 0x7F) as usize) << shift;
                    if (b & 0x80) == 0 {
                        break;
                    }
                    shift += 7;
                    if shift > 28 {
                        break;
                    }
                }

                if count > 0 && mem_pos < mem_end {
                    // Read limits flag
                    let _flag = bytes[mem_pos];
                    mem_pos += 1;

                    // Read initial page limit (LEB128)
                    let mut initial_pages = 0usize;
                    let mut shift = 0;
                    while mem_pos < mem_end {
                        let b = bytes[mem_pos];
                        mem_pos += 1;
                        initial_pages |= ((b & 0x7F) as usize) << shift;
                        if (b & 0x80) == 0 {
                            break;
                        }
                        shift += 7;
                        if shift > 28 {
                            break;
                        }
                    }
                    declared_memory = Some((initial_pages.max(1)) as u32);
                } else {
                    declared_memory = Some(1);
                }
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

/// Static WebAssembly Module Inspector and Header Validator.
///
/// NOTE: This tool validates and inspects WASM module structure (magic header, version,
/// declared memory pages, and section exports). It is a static inspector and validator stub,
/// NOT an active runtime WASM bytecode execution sandbox.
pub struct WasmInspector {
    name: String,
    description: String,
    input_schema: Value,
    wasm_bytes: Vec<u8>,
    config: WasmSandboxConfig,
    invocations: AtomicU64,
}

/// Backwards compatibility alias for `WasmInspector`
pub type WasmTool = WasmInspector;

impl WasmInspector {
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
impl Tool for WasmInspector {
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

        let metadata = WasmModuleValidator::inspect(&self.wasm_bytes, &self.config)?;

        let output = json!({
            "status": "inspected_wasm_module_metadata",
            "inspector_mode": "static_validation_stub",
            "module_size": self.wasm_bytes.len(),
            "declared_memory_pages": metadata.declared_memory_pages,
            "export_count": metadata.export_count,
            "max_memory_pages": self.config.max_memory_pages,
            "arguments_echo": arguments,
            "note": "Static metadata inspection only. Bytecode execution is not supported in this stub inspector."
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
