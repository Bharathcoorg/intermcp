use serde_json::{json, Value};
use std::env;
use std::time::Duration;
use tokio::process::Command;

use crate::error::FastMcpError;
use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};

const DEFAULT_ALLOWED_BINARIES: &[&str] = &[
    "git", "ls", "cat", "grep", "echo", "pwd", "npm", "node", "python", "python3", "curl", "rg",
];

pub const SAFE_ENV_VARS: &[&str] = &[
    "PATH",
    "Path",
    "SYSTEMROOT",
    "SystemRoot",
    "TEMP",
    "TMP",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERPROFILE",
    "HOME",
    "LANG",
    "LC_ALL",
    "TERM",
];

pub fn apply_isolated_environment(cmd: &mut Command) {
    cmd.env_clear();
    for &key in SAFE_ENV_VARS {
        if let Ok(val) = env::var(key) {
            cmd.env(key, val);
        }
    }
}

pub fn create_system_info_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "system_info",
        "Retrieve host system architecture, operating system, and hardware environment diagnostics",
        json!({
            "type": "object",
            "properties": {}
        }),
        |_args: Value| async move {
            let os = env::consts::OS;
            let arch = env::consts::ARCH;
            let current_dir = env::current_dir().unwrap_or_default().to_string_lossy().to_string();

            let info = json!({
                "os": os,
                "arch": arch,
                "currentWorkingDir": current_dir,
                "processId": std::process::id(),
                "rustRuntime": "Pure Native Rust Engine (InterMCP)",
                "memoryOverhead": "< 4MB RSS",
            });

            Ok(CallToolResult::text(serde_json::to_string_pretty(&info).unwrap_or_default()))
        },
    ).with_cacheable(true))
}

fn get_path_dirs() -> &'static [std::path::PathBuf] {
    static PATH_DIRS: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH_DIRS.get_or_init(|| {
        let path_var = env::var_os("PATH")
            .or_else(|| env::var_os("Path"))
            .unwrap_or_default();
        env::split_paths(&path_var).collect()
    })
}

pub fn resolve_binary_in_path(binary: &str) -> Option<std::path::PathBuf> {
    for dir in get_path_dirs() {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{}.exe", binary));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
            let candidate_cmd = dir.join(format!("{}.cmd", binary));
            if candidate_cmd.is_file() {
                return Some(candidate_cmd);
            }
            let candidate_bat = dir.join(format!("{}.bat", binary));
            if candidate_bat.is_file() {
                return Some(candidate_bat);
            }
        }
    }
    None
}

fn contains_unquoted_shell_meta(cmd: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;

    for c in cmd.chars() {
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        } else if !in_single
            && !in_double
            && matches!(c, ';' | '&' | '|' | '\n' | '\r' | '(' | ')' | '<' | '>')
        {
            return true;
        }
    }
    false
}

pub fn create_shell_exec_tool() -> Box<dyn Tool> {
    create_shell_exec_tool_with_allowlist(Vec::new())
}

pub fn create_shell_exec_tool_with_allowlist(extra_allowed: Vec<String>) -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "system_run_command",
        "Execute a safe terminal command and return stdout/stderr with a 30-second timeout",
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run (e.g. 'git status' or 'cargo check')" }
            },
            "required": ["command"]
        }),
        move |args: Value| {
            let extra = extra_allowed.clone();
            async move {
                let cmd_str = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| FastMcpError::InvalidRequest("Missing command".into()))?;

                if contains_unquoted_shell_meta(cmd_str)
                    || cmd_str.contains('`')
                    || cmd_str.contains("$(")
                {
                    return Ok(CallToolResult::error(
                        "Safe-Shell Violation: Chained/shell-meta commands are prohibited. Invoke each command in a separate tools/call. Execution blocked by security policy.".to_string(),
                    ));
                }

                if let Err(violation) = validate_shell_command(cmd_str, &extra) {
                    return Ok(CallToolResult::error(format!(
                        "Safe-Shell Violation: {}. Execution blocked by security policy.",
                        violation
                    )));
                }

                let tokens = match tokenize(cmd_str) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(CallToolResult::error(format!(
                            "Safe-Shell Violation: {}",
                            e
                        )))
                    }
                };
                if tokens.is_empty() {
                    return Ok(CallToolResult::error("Empty command".to_string()));
                }

                #[cfg(target_os = "windows")]
                let is_builtin = matches!(
                    tokens[0].to_lowercase().as_str(),
                    "echo" | "dir" | "type" | "cls" | "cd"
                );
                #[cfg(not(target_os = "windows"))]
                let is_builtin = false;

                let mut cmd = if is_builtin {
                    #[cfg(target_os = "windows")]
                    {
                        let mut c = Command::new("cmd");
                        c.arg("/D").arg("/C").args(&tokens);
                        c
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        let mut c = Command::new(&tokens[0]);
                        if tokens.len() > 1 {
                            c.args(&tokens[1..]);
                        }
                        c
                    }
                } else {
                    let mut c = Command::new(&tokens[0]);
                    if tokens.len() > 1 {
                        c.args(&tokens[1..]);
                    }
                    c
                };

                apply_isolated_environment(&mut cmd);
                crate::reaper::configure_child_isolation(&mut cmd);

                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(CallToolResult::error(format!(
                            "Command failed to start: {}",
                            e
                        )));
                    }
                };

                let mut guard = match crate::reaper::ChildIsolationGuard::new(&child) {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = child.kill().await;
                        return Ok(CallToolResult::error(format!(
                            "Failed to establish process isolation: {}",
                            e
                        )));
                    }
                };
                let timeout_duration = Duration::from_secs(30);

                let stdout_handle = child.stdout.take();
                let stderr_handle = child.stderr.take();

                let mut stdout_buf = bytes::BytesMut::with_capacity(64 * 1024);
                let mut stderr_buf = bytes::BytesMut::with_capacity(64 * 1024);

                const MAX_READ_CAP: u64 = 32 * 1024 * 1024;

                let out_fut = async {
                    if let Some(h) = stdout_handle {
                        use tokio::io::AsyncReadExt;
                        let mut limited = h.take(MAX_READ_CAP);
                        let mut chunk = [0u8; 8192];
                        while let Ok(n) = limited.read(&mut chunk).await {
                            if n == 0 {
                                break;
                            }
                            stdout_buf.extend_from_slice(&chunk[..n]);
                        }
                    }
                };

                let err_fut = async {
                    if let Some(h) = stderr_handle {
                        use tokio::io::AsyncReadExt;
                        let mut limited = h.take(MAX_READ_CAP);
                        let mut chunk = [0u8; 8192];
                        while let Ok(n) = limited.read(&mut chunk).await {
                            if n == 0 {
                                break;
                            }
                            stderr_buf.extend_from_slice(&chunk[..n]);
                        }
                    }
                };

                let stream_fut = async {
                    tokio::join!(out_fut, err_fut);
                    child.wait().await
                };

                match tokio::time::timeout(timeout_duration, stream_fut).await {
                    Ok(status_res) => match status_res {
                        Ok(status) => {
                            guard.disarm();
                            let mut stdout = String::from_utf8_lossy(&stdout_buf).to_string();
                            let mut stderr = String::from_utf8_lossy(&stderr_buf).to_string();

                            const MAX_OUTPUT_CHARS: usize = 256 * 1024;
                            if stdout.len() > MAX_OUTPUT_CHARS {
                                stdout.truncate(MAX_OUTPUT_CHARS);
                                stdout.push_str("\n... [Output truncated: exceeded 256KB]");
                            }
                            if stderr.len() > MAX_OUTPUT_CHARS {
                                stderr.truncate(MAX_OUTPUT_CHARS);
                                stderr.push_str("\n... [Stderr truncated: exceeded 256KB]");
                            }

                            let exit_code = status.code().unwrap_or(-1);
                            let res = json!({
                                "exitCode": exit_code,
                                "stdout": stdout,
                                "stderr": stderr
                            });

                            Ok(CallToolResult::text(
                                serde_json::to_string_pretty(&res).unwrap_or_default(),
                            ))
                        }
                        Err(e) => {
                            guard.kill_group();
                            Ok(CallToolResult::error(format!("Command failed: {}", e)))
                        }
                    },
                    Err(_) => {
                        let _ = child.kill().await;
                        guard.kill_group();
                        Ok(CallToolResult::error(
                            "Execution timed out after 30 seconds",
                        ))
                    }
                }
            }
        },
    ))
}

pub fn validate_shell_command(cmd: &str, extra_allowed: &[String]) -> Result<(), String> {
    let raw = cmd.trim();
    if raw.is_empty() {
        return Err("Empty command".into());
    }

    if raw.contains("$(") {
        return Err("Command substitution using '$(' is prohibited. Chained/redirected/parenthesized commands are prohibited.".into());
    }
    if raw.contains('`') {
        return Err("Command substitution using backticks (`) is prohibited. Chained/redirected/parenthesized commands are prohibited.".into());
    }
    if raw.contains("${") {
        return Err("Variable expansion using '${' is prohibited".into());
    }
    if contains_tilde_expansion(raw) {
        return Err("Tilde expansion (~) is prohibited".into());
    }
    if contains_unquoted_shell_meta(raw) {
        return Err("Chained/redirected/parenthesized commands are prohibited. Invoke each command in a separate tools/call.".into());
    }

    if raw.contains(":(){ :|:& };:") || raw.contains(":(){:|:&};:") {
        return Err("Fork bomb detected".into());
    }

    let collapsed: Vec<&str> = raw.split_whitespace().collect();
    if !collapsed.is_empty()
        && (collapsed[0] == "rm" || collapsed[0].ends_with("/rm") || collapsed[0].ends_with("\\rm"))
    {
        let destructive_flags = [
            "-rf",
            "-fr",
            "-r",
            "-f",
            "-R",
            "-Rf",
            "-RF",
            "--recursive",
            "--force",
        ];
        let has_destructive_flag = collapsed[1..]
            .iter()
            .any(|&t| destructive_flags.contains(&t));
        let mut has_positional = false;
        let mut past_dashdash = false;
        for &tok in &collapsed[1..] {
            if past_dashdash {
                has_positional = true;
                break;
            }
            if tok == "--" {
                past_dashdash = true;
                continue;
            }
            if !tok.starts_with('-') {
                has_positional = true;
                break;
            }
        }
        if has_destructive_flag && has_positional {
            return Err("Destructive recursive deletion (rm -rf) is prohibited".into());
        }
    }

    let tokens = tokenize(raw)?;
    if tokens.is_empty() {
        return Err("Empty command".into());
    }

    let raw_binary = &tokens[0];
    if raw_binary.contains('=') {
        return Err(
            "Inline environment variable assignment in command prefix is prohibited".into(),
        );
    }

    let normalized_raw = raw_binary.to_lowercase().replace('\\', "/");
    if normalized_raw == "/usr/bin/env"
        || normalized_raw == "/usr/bin/env.exe"
        || normalized_raw == "env"
        || normalized_raw == "env.exe"
        || normalized_raw.ends_with("/env")
        || normalized_raw.ends_with("/env.exe")
    {
        return Err(
            "Use of /usr/bin/env is prohibited to prevent PATH-driven binary escalation".into(),
        );
    }

    let has_path_separator = raw_binary.contains('/') || raw_binary.contains('\\');
    if has_path_separator {
        let is_explicitly_allowed_path = extra_allowed
            .iter()
            .any(|b| b.eq_ignore_ascii_case(raw_binary) || b.eq_ignore_ascii_case(&normalized_raw));
        if !is_explicitly_allowed_path {
            return Err(format!(
                "Path-qualified executable '{}' is prohibited. Direct path execution is restricted to prevent binary hijacking.",
                raw_binary
            ));
        }
    }

    let normalized_binary = extract_binary_name(raw_binary);

    let is_allowed = DEFAULT_ALLOWED_BINARIES
        .iter()
        .any(|&b| b.eq_ignore_ascii_case(&normalized_binary))
        || extra_allowed
            .iter()
            .any(|b| b.eq_ignore_ascii_case(&normalized_binary));

    if !is_allowed {
        return Err(format!(
            "Binary '{}' is not in the execution allowlist",
            normalized_binary
        ));
    }

    #[cfg(target_os = "windows")]
    let is_builtin = matches!(
        normalized_binary.to_lowercase().as_str(),
        "echo" | "dir" | "type" | "cls" | "cd"
    );
    #[cfg(not(target_os = "windows"))]
    let is_builtin = false;

    if !is_builtin && !has_path_separator {
        if let Some(resolved) = resolve_binary_in_path(&normalized_binary) {
            let stem = resolved
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let base_matches = DEFAULT_ALLOWED_BINARIES
                .iter()
                .any(|&b| b.eq_ignore_ascii_case(&stem))
                || extra_allowed.iter().any(|b| b.eq_ignore_ascii_case(&stem));
            if !base_matches {
                return Err(format!(
                    "Binary '{}' resolved in PATH as '{}' which does not match allowlist",
                    normalized_binary,
                    resolved.display()
                ));
            }
        } else {
            let base_matches = DEFAULT_ALLOWED_BINARIES
                .iter()
                .any(|&b| b.eq_ignore_ascii_case(&normalized_binary))
                || extra_allowed
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case(&normalized_binary));
            if !base_matches {
                return Err(format!(
                    "Binary '{}' not allowed for PATH-search execution",
                    normalized_binary
                ));
            }
        }
    }

    if normalized_binary.eq_ignore_ascii_case("rm") {
        let mut has_r = false;
        let mut has_f = false;
        for token in &tokens[1..] {
            let t = token.trim();
            match t {
                "-rf" | "-fr" | "-Rf" | "-RF" => {
                    return Err("Destructive recursive deletion (rm -rf) is prohibited".into());
                }
                "-r" | "-R" | "--recursive" => {
                    has_r = true;
                }
                "-f" | "--force" => {
                    has_f = true;
                }
                _ => {}
            }
            if has_r && has_f {
                return Err("Destructive recursive deletion (rm -rf) is prohibited".into());
            }
        }
    }

    if normalized_binary.eq_ignore_ascii_case("git") {
        let lower_cmd = raw.to_lowercase();
        if lower_cmd.contains("core.editor")
            || lower_cmd.contains("core.pager")
            || lower_cmd.contains("--upload-pack")
            || lower_cmd.contains("receive.fsck")
        {
            if lower_cmd.contains("rm -rf") {
                return Err(
                    "Git option injection with destructive command (rm -rf) is prohibited".into(),
                );
            }
            return Err("Git option injection (core.editor / execution hook) is prohibited".into());
        }
    }

    let lower_tokens: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    let joined_sub = lower_tokens.join(" ");

    let has_opt_in =
        |flag: &str| -> bool { extra_allowed.iter().any(|b| b.eq_ignore_ascii_case(flag)) };

    if (normalized_binary.eq_ignore_ascii_case("python")
        || normalized_binary.eq_ignore_ascii_case("python3"))
        && lower_tokens.iter().any(|t| t == "-c")
        && !has_opt_in("python -c")
        && !has_opt_in("python3 -c")
    {
        return Err("Arbitrary code execution flag 'python -c' is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("perl")
        && lower_tokens.iter().any(|t| t == "-e")
        && !has_opt_in("perl -e")
    {
        return Err("Arbitrary code execution flag 'perl -e' is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("ruby")
        && lower_tokens.iter().any(|t| t == "-e")
        && !has_opt_in("ruby -e")
    {
        return Err("Arbitrary code execution flag 'ruby -e' is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("node")
        && lower_tokens.iter().any(|t| t == "-e" || t == "--eval")
        && !has_opt_in("node -e")
    {
        return Err("Arbitrary code execution flag 'node -e' is prohibited".into());
    }

    if (normalized_binary.eq_ignore_ascii_case("powershell")
        || normalized_binary.eq_ignore_ascii_case("pwsh"))
        && lower_tokens
            .iter()
            .any(|t| t == "-encodedcommand" || t == "-e")
    {
        return Err("PowerShell -EncodedCommand is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("find")
        && (joined_sub.contains("-delete") || joined_sub.contains("-exec rm"))
    {
        return Err("Destructive find execution (-delete or -exec rm) is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("rsync") && joined_sub.contains("--delete") {
        return Err("Destructive rsync execution (--delete) is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("mv")
        && (joined_sub.contains("/dev/null") || joined_sub.contains("/*"))
    {
        return Err("Destructive move to /dev/null is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("chmod")
        && (joined_sub.contains("-r 000") || joined_sub.contains("000 /"))
    {
        return Err("Destructive permission zeroing (chmod 000) is prohibited".into());
    }

    if (normalized_binary.eq_ignore_ascii_case("rd")
        || normalized_binary.eq_ignore_ascii_case("rmdir"))
        && (joined_sub.contains("/s") || joined_sub.contains("-s"))
    {
        return Err("Destructive recursive directory removal is prohibited".into());
    }

    if normalized_binary.eq_ignore_ascii_case("format")
        || normalized_binary.eq_ignore_ascii_case("diskpart")
        || (normalized_binary.eq_ignore_ascii_case("cipher") && joined_sub.contains("/w"))
    {
        return Err("Disk destruction/formatting command is prohibited".into());
    }

    if raw.contains("/dev/sd")
        || raw.contains("/dev/nvme")
        || raw.contains("/dev/hd")
        || raw.contains("/dev/disk")
    {
        return Err("Direct raw block device access or modification is prohibited".into());
    }

    if (normalized_binary.eq_ignore_ascii_case("curl")
        || normalized_binary.eq_ignore_ascii_case("wget")
        || normalized_binary.eq_ignore_ascii_case("base64"))
        && (raw.contains("| sh")
            || raw.contains("| bash")
            || raw.contains("|sh")
            || raw.contains("|bash")
            || raw.contains("| zsh")
            || raw.contains("| powershell")
            || raw.contains("| cmd"))
    {
        return Err(
            "Unchecked remote code execution pipeline (curl/base64 | sh) is prohibited".into(),
        );
    }

    if joined_sub.contains("/dev/tcp/")
        || (normalized_binary.eq_ignore_ascii_case("nc")
            && (joined_sub.contains("-e /bin/sh") || joined_sub.contains("-e /bin/bash")))
    {
        return Err("Reverse shell pattern detected".into());
    }

    Ok(())
}

pub(crate) fn split_chained_commands(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let chars: Vec<char> = cmd.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];
        if c == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(c);
        } else if c == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(c);
        } else if !in_single_quote && !in_double_quote {
            if c == ';' || c == '\n' {
                parts.push(std::mem::take(&mut current));
            } else if i + 1 < len
                && ((c == '&' && chars[i + 1] == '&') || (c == '|' && chars[i + 1] == '|'))
            {
                parts.push(std::mem::take(&mut current));
                i += 1;
            } else if c == '|' || c == '&' {
                parts.push(std::mem::take(&mut current));
            } else {
                current.push(c);
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.trim().is_empty() {
        parts.push(current);
    }

    parts
}

fn contains_tilde_expansion(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '~' {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = if i + 1 < chars.len() {
                Some(chars[i + 1])
            } else {
                None
            };
            match prev {
                None => return true,
                Some(p)
                    if p.is_whitespace()
                        || p == '"'
                        || p == '\''
                        || p == '='
                        || p == ':'
                        || p == ';'
                        || p == '|'
                        || p == '&'
                        || p == '/' =>
                {
                    return true;
                }
                _ => {}
            }
            if let Some('/') = next {
                return true;
            }
            if next.is_none() || next.unwrap().is_whitespace() {
                return true;
            }
        }
    }
    false
}

fn tokenize(cmd: &str) -> Result<Vec<String>, String> {
    if cmd.contains("$(") {
        return Err("Command substitution using '$(' is prohibited".into());
    }
    if cmd.contains('`') {
        return Err("Command substitution using backticks (`) is prohibited".into());
    }
    if cmd.contains("${") {
        return Err("Variable expansion using '${' is prohibited".into());
    }
    if contains_tilde_expansion(cmd) {
        return Err("Tilde expansion (~) is prohibited".into());
    }

    shell_words::split(cmd).map_err(|e| format!("Invalid shell syntax: {}", e))
}

fn extract_binary_name(raw: &str) -> String {
    let mut s = raw.trim();
    let p = std::path::Path::new(s);
    if s.starts_with('/') {
        if let Some(name) = p.file_name().and_then(|f| f.to_str()) {
            return name.strip_suffix(".exe").unwrap_or(name).to_string();
        }
    }
    while s.starts_with('\\') || s.starts_with('/') {
        s = &s[1..];
    }

    let last_segment = s.rsplit(['/', '\\']).next().unwrap_or(s);
    let p = std::path::Path::new(last_segment);
    let stem = p
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap_or(last_segment);

    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_chained_semicolon() {
        let res = validate_shell_command("git status; echo pwned", &[]);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Chained/redirected/parenthesized commands are prohibited"));
    }

    #[test]
    fn validate_rejects_rm_rf_with_dashdash() {
        let res = validate_shell_command("rm -rf -- target", &[]);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Destructive recursive deletion (rm -rf) is prohibited"));
    }

    #[test]
    fn validate_accepts_plain_git_status() {
        let res = validate_shell_command("git status", &[]);
        assert!(
            res.is_ok(),
            "Expected git status to be allowed, got {:?}",
            res
        );
    }
}
