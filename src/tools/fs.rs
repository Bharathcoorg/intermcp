use serde_json::{json, Value};
use std::fs;
use std::path::Path;

use crate::error::FastMcpError;
use crate::protocol::CallToolResult;
use crate::sandbox::{HardlinkExt, SandboxPolicy};
use crate::tool::{SimpleTool, Tool};

pub fn create_fs_read_tool(sandbox: SandboxPolicy) -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "fs_read_file",
        "Read the text content of a local file safely with SafeFS security boundaries",
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to read"
                }
            },
            "required": ["path"]
        }),
        move |args: Value| {
            let sb = sandbox.clone();
            async move {
                let path_str = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    FastMcpError::InvalidRequest("Missing required 'path' parameter".into())
                })?;

                let path = Path::new(path_str);
                let safe_path = match sb.validate_path(path) {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::error(e.to_string())),
                };

                if !safe_path.exists() {
                    return Ok(CallToolResult::error(format!(
                        "File not found: {}",
                        path_str
                    )));
                }

                if let Ok(sym_meta) = safe_path.symlink_metadata() {
                    if sym_meta.file_type().is_symlink() {
                        return Ok(CallToolResult::error(
                            "Security error: Symlink access is prohibited".to_string(),
                        ));
                    }
                    if sym_meta.file_type().is_hardlink() || sym_meta.is_hardlink() {
                        return Ok(CallToolResult::error(
                            "SafeFS Violation: Hardlink detected at target. Hardlinks are prohibited.".to_string(),
                        ));
                    }
                }

                // Protect against out-of-memory crashes on massive files (> 10MB)
                const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;
                if let Ok(metadata) = safe_path.metadata() {
                    if metadata.len() > MAX_READ_BYTES {
                        return Ok(CallToolResult::error(format!(
                            "File size ({:.2} MB) exceeds maximum allowed read limit (10 MB). Refusing to load to prevent memory exhaustion.",
                            metadata.len() as f64 / (1024.0 * 1024.0)
                        )));
                    }
                }

                match fs::read_to_string(&safe_path) {
                    Ok(content) => Ok(CallToolResult::text(content)),
                    Err(e) => Ok(CallToolResult::error(format!("Failed to read file: {}", e))),
                }
            }
        },
    ))
}

pub fn create_fs_write_tool(sandbox: SandboxPolicy) -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "fs_write_file",
        "Write text content to a local file safely with directory tree creation and SafeFS validation",
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The destination file path"
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write to the file"
                }
            },
            "required": ["path", "content"]
        }),
        move |args: Value| {
            let sb = sandbox.clone();
            async move {
                let path_str = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| FastMcpError::InvalidRequest("Missing 'path' parameter".into()))?;

                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| FastMcpError::InvalidRequest("Missing 'content' parameter".into()))?;

                let path = Path::new(path_str);
                let safe_path = match sb.validate_path(path) {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::error(e.to_string())),
                };

                if let Ok(sym_meta) = safe_path.symlink_metadata() {
                    if sym_meta.file_type().is_symlink() {
                        return Ok(CallToolResult::error(
                            "Security error: Cannot overwrite symlink target".to_string(),
                        ));
                    }
                    if sym_meta.file_type().is_hardlink() || sym_meta.is_hardlink() {
                        return Ok(CallToolResult::error(
                            "SafeFS Violation: Cannot overwrite hardlink target. Hardlinks are prohibited.".to_string(),
                        ));
                    }
                }

                if let Some(parent) = safe_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                match fs::write(&safe_path, content) {
                    Ok(_) => Ok(CallToolResult::text(format!(
                        "Successfully wrote {} bytes to {}",
                        content.len(),
                        path_str
                    ))),
                    Err(e) => Ok(CallToolResult::error(format!("Failed to write file: {}", e))),
                }
            }
        },
    ))
}

pub fn create_fs_list_tool(sandbox: SandboxPolicy) -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "fs_list_dir",
        "List files and subdirectories within a specified directory path",
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path of the directory to list (defaults to current directory if omitted)"
                }
            }
        }),
        move |args: Value| {
            let sb = sandbox.clone();
            async move {
                let target_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

                let safe_path = match sb.validate_path(Path::new(target_path)) {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::error(e.to_string())),
                };

                if let Ok(sym_meta) = safe_path.symlink_metadata() {
                    if sym_meta.file_type().is_symlink() {
                        return Ok(CallToolResult::error(
                            "Security error: Symlink access is prohibited".to_string(),
                        ));
                    }
                    if sym_meta.file_type().is_hardlink() || sym_meta.is_hardlink() {
                        return Ok(CallToolResult::error(
                            "SafeFS Violation: Hardlink detected at target. Hardlinks are prohibited.".to_string(),
                        ));
                    }
                }

                let read_dir = match fs::read_dir(&safe_path) {
                    Ok(rd) => rd,
                    Err(e) => {
                        return Ok(CallToolResult::error(format!(
                            "Cannot list directory {}: {}",
                            target_path, e
                        )))
                    }
                };

                let mut entries = Vec::new();
                for entry in read_dir.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    let file_type = if entry.path().is_dir() { "dir" } else { "file" };
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    entries.push(json!({
                        "name": file_name,
                        "type": file_type,
                        "sizeBytes": size
                    }));
                }

                Ok(CallToolResult::text(
                    serde_json::to_string_pretty(&entries).unwrap_or_default(),
                ))
            }
        },
    ))
}

pub fn create_fs_search_tool(sandbox: SandboxPolicy) -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "fs_search_text",
        "Fast recursive pattern search for keywords inside project files (ripgrep style)",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Text or keyword to search for" },
                "dir": { "type": "string", "description": "Directory to search within (defaults to '.')" }
            },
            "required": ["query"]
        }),
        move |args: Value| {
            let sb = sandbox.clone();
            async move {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let dir = args.get("dir").and_then(|v| v.as_str()).unwrap_or(".");

                if query.is_empty() {
                    return Ok(CallToolResult::error("Search query cannot be empty"));
                }

                let safe_dir = match sb.validate_path(Path::new(dir)) {
                    Ok(p) => p,
                    Err(e) => return Ok(CallToolResult::error(e.to_string())),
                };

                let mut matches = Vec::new();
                search_dir(&safe_dir, query, &mut matches, 0);

                let result = json!({
                    "query": query,
                    "totalMatches": matches.len(),
                    "matches": matches
                });

                Ok(CallToolResult::text(
                    serde_json::to_string_pretty(&result).unwrap_or_default(),
                ))
            }
        },
    ))
}

fn search_dir(dir: &Path, query: &str, matches: &mut Vec<Value>, depth: usize) {
    if depth > 10 || matches.len() >= 50 {
        return;
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();

            // Skip symlinks to prevent infinite recursive cycles
            if p.is_symlink() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name == "dist"
                || name == "build"
                || name == "venv"
                || name == "__pycache__"
            {
                continue;
            }

            if p.is_dir() {
                search_dir(&p, query, matches, depth + 1);
            } else if p.is_file() {
                // Skip binary files by common file extension
                if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if matches!(
                        ext_lower.as_str(),
                        "png"
                            | "jpg"
                            | "jpeg"
                            | "gif"
                            | "ico"
                            | "webp"
                            | "pdf"
                            | "zip"
                            | "tar"
                            | "gz"
                            | "exe"
                            | "dll"
                            | "so"
                            | "dylib"
                            | "wasm"
                            | "bin"
                            | "db"
                            | "sqlite"
                            | "woff"
                            | "woff2"
                            | "ttf"
                            | "eot"
                    ) {
                        continue;
                    }
                }

                // Skip credential or secret files protected by Secret Shield
                if SandboxPolicy::is_sensitive_path_default(&p) {
                    continue;
                }

                // Skip files larger than 2MB to prevent memory exhaustion and UI lag
                if let Ok(meta) = entry.metadata() {
                    if meta.len() > 2 * 1024 * 1024 {
                        continue;
                    }
                }

                if let Ok(content) = fs::read_to_string(&p) {
                    for (line_no, line) in content.lines().enumerate() {
                        if line.contains(query) {
                            matches.push(json!({
                                "file": p.to_string_lossy(),
                                "line": line_no + 1,
                                "preview": line.trim()
                            }));
                            if matches.len() >= 50 {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}
