use crate::error::FastMcpError;
use std::path::{Component, Path, PathBuf};

#[cfg(windows)]
pub mod win32 {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::RawHandle;
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE,
        FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    pub fn check_no_reparse_point_and_no_hardlink(
        path: &Path,
    ) -> Result<(bool, bool), std::io::Error> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);

        // # Safety
        // Calls Win32 CreateFileW with PCWSTR pointing to a valid null-terminated UTF-16 buffer.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_GENERIC_READ.0,
                FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(
                    FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0,
                ),
                HANDLE::default(),
            )
        };

        match handle {
            Ok(h) => {
                let mut info = BY_HANDLE_FILE_INFORMATION::default();
                // # Safety
                // `h` is a valid handle and `info` points to a stack-allocated BY_HANDLE_FILE_INFORMATION.
                let res = unsafe { GetFileInformationByHandle(h, &mut info) };
                // # Safety
                // `h` is an open Win32 HANDLE that is closed once.
                unsafe {
                    let _ = CloseHandle(h);
                };

                if res.is_ok() {
                    let is_reparse = (info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0;
                    let is_hardlink = info.nNumberOfLinks > 1;
                    Ok((is_reparse, is_hardlink))
                } else {
                    Err(std::io::Error::last_os_error())
                }
            }
            Err(e) => Err(std::io::Error::from_raw_os_error(e.code().0)),
        }
    }

    pub fn check_is_hardlink(path: &Path) -> Result<bool, std::io::Error> {
        check_no_reparse_point_and_no_hardlink(path).map(|(_, is_hl)| is_hl)
    }

    pub fn check_is_hardlink_handle(raw_handle: RawHandle) -> Result<bool, std::io::Error> {
        let handle = HANDLE(raw_handle as _);
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // # Safety
        // `handle` is checked and borrowed from an active File; info is a valid pointer.
        let res = unsafe { GetFileInformationByHandle(handle, &mut info) };
        if res.is_ok() {
            Ok(info.nNumberOfLinks > 1)
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    pub fn open_new_or_verify_reparse_point(path: &Path) -> Result<(), std::io::Error> {
        if path.exists() {
            let (is_reparse, is_hardlink) = check_no_reparse_point_and_no_hardlink(path)?;
            if is_reparse {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Reparse point / symlink target prohibited",
                ));
            }
            if is_hardlink {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Hardlink target prohibited",
                ));
            }
            Ok(())
        } else {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);

            // # Safety
            // Calls Win32 CreateFileW with CREATE_NEW to atomically create a new file and reject existing symlinks.
            let handle = unsafe {
                CreateFileW(
                    PCWSTR(wide.as_ptr()),
                    FILE_GENERIC_WRITE.0,
                    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0),
                    None,
                    CREATE_NEW,
                    FILE_FLAGS_AND_ATTRIBUTES(FILE_FLAG_OPEN_REPARSE_POINT.0),
                    HANDLE::default(),
                )
            };

            match handle {
                Ok(h) => {
                    // # Safety
                    // `h` is safely closed.
                    unsafe {
                        let _ = CloseHandle(h);
                    };
                    Ok(())
                }
                Err(e) => Err(std::io::Error::from_raw_os_error(e.code().0)),
            }
        }
    }
}

pub trait HardlinkExt {
    fn is_hardlink(&self) -> bool;
}

impl HardlinkExt for Path {
    fn is_hardlink(&self) -> bool {
        #[cfg(unix)]
        {
            if let Ok(meta) = std::fs::symlink_metadata(self) {
                use std::os::unix::fs::MetadataExt;
                return meta.is_file() && meta.nlink() > 1;
            }
            false
        }
        #[cfg(windows)]
        {
            win32::check_is_hardlink(self).unwrap_or(false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    }
}

impl HardlinkExt for PathBuf {
    fn is_hardlink(&self) -> bool {
        self.as_path().is_hardlink()
    }
}

impl HardlinkExt for std::fs::Metadata {
    fn is_hardlink(&self) -> bool {
        if !self.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.nlink() > 1
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

#[inline]
fn path_components_equal(a: &Path, b: &Path) -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let a_str = a.as_os_str().to_string_lossy();
        let b_str = b.as_os_str().to_string_lossy();
        a_str.eq_ignore_ascii_case(&b_str)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        a == b
    }
}

const DEFAULT_SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    ".netrc",
    ".pgpass",
    ".bash_history",
    ".zsh_history",
    "kubeconfig",
    "credentials.json",
    "service_account.json",
    "vault.json",
    ".npmrc",
    "secrets.json",
    "secret.json",
    "token.json",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];

const DEFAULT_SENSITIVE_KEYWORDS: &[&str] = &[
    "secret",
    "token",
    "key",
    "password",
    "credential",
    "mnemonic",
    "seed",
    "wallet",
    "private",
];

const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key", "pkcs12", "pfx"];

#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    allowed_roots: Vec<PathBuf>,
    enabled: bool,
    shield_secrets: bool,
    custom_sensitive_files: Vec<String>,
    custom_sensitive_keywords: Vec<String>,
}

impl SandboxPolicy {
    pub fn unrestricted() -> Self {
        Self {
            allowed_roots: Vec::new(),
            enabled: false,
            shield_secrets: true,
            custom_sensitive_files: Vec::new(),
            custom_sensitive_keywords: Vec::new(),
        }
    }

    pub fn new(roots: Vec<PathBuf>) -> Self {
        let mut canonical_roots: Vec<PathBuf> = Vec::new();
        for p in roots {
            if let Ok(c) = dunce::canonicalize(&p) {
                if !canonical_roots.iter().any(|r| path_components_equal(r, &c)) {
                    canonical_roots.push(c);
                }
            }
            if !canonical_roots.iter().any(|r| path_components_equal(r, &p)) {
                canonical_roots.push(p);
            }
        }

        Self {
            allowed_roots: canonical_roots,
            enabled: true,
            shield_secrets: true,
            custom_sensitive_files: Vec::new(),
            custom_sensitive_keywords: Vec::new(),
        }
    }

    pub fn with_secret_shield(mut self, enabled: bool) -> Self {
        self.shield_secrets = enabled;
        self
    }

    pub fn with_additional_sensitive_files(mut self, files: Vec<String>) -> Self {
        self.custom_sensitive_files.extend(files);
        self
    }

    pub fn with_additional_sensitive_keywords(mut self, keywords: Vec<String>) -> Self {
        self.custom_sensitive_keywords.extend(keywords);
        self
    }

    pub fn is_reserved_device_name(name: &str) -> bool {
        let stem = match name.split('.').next() {
            Some(s) => s.to_ascii_uppercase(),
            None => return false,
        };
        matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "CLOCK$"
                | "CONIN$"
                | "CONOUT$"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
    }

    pub fn is_windows_short_name(name: &str) -> bool {
        if let Some(pos) = name.find('~') {
            let rest = &name[pos + 1..];
            let digits = rest.split('.').next().unwrap_or("");
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
            if rest
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }

    pub fn is_sensitive_path(&self, path: &Path) -> bool {
        Self::check_sensitive_path(
            path,
            &self.custom_sensitive_files,
            &self.custom_sensitive_keywords,
        )
    }

    pub fn is_sensitive_path_default(path: &Path) -> bool {
        Self::check_sensitive_path(path, &[], &[])
    }

    pub fn check_sensitive_path(
        path: &Path,
        custom_files: &[String],
        custom_keywords: &[String],
    ) -> bool {
        let path_str = path.to_string_lossy().to_lowercase().replace('\\', "/");

        if path_str.contains(".docker/config.json") || path_str.contains("gcloud/credentials.db") {
            return true;
        }

        for component in path.components() {
            if let Component::Normal(c) = component {
                let segment = c.to_string_lossy().to_lowercase();
                if segment == ".ssh"
                    || segment == ".aws"
                    || segment == ".git"
                    || segment == ".gnupg"
                    || segment == ".docker"
                {
                    return true;
                }
            }
        }

        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            let lower_name = file_name.to_lowercase();
            for sensitive in DEFAULT_SENSITIVE_FILES {
                if lower_name == *sensitive || lower_name.starts_with(".env.") {
                    return true;
                }
            }
            for custom in custom_files {
                if lower_name == custom.to_lowercase() {
                    return true;
                }
            }

            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let lower_stem = stem.to_lowercase();
                for kw in DEFAULT_SENSITIVE_KEYWORDS {
                    if lower_stem.contains(kw) {
                        return true;
                    }
                }
                for kw in custom_keywords {
                    if lower_stem.contains(&kw.to_lowercase()) {
                        return true;
                    }
                }
            }

            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                for sensitive_ext in SENSITIVE_EXTENSIONS {
                    if ext_lower == *sensitive_ext {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn validate_path(&self, requested: &Path) -> Result<PathBuf, FastMcpError> {
        let raw_str = requested.to_string_lossy();
        if raw_str.contains('\0') {
            return Err(FastMcpError::ToolExecution(
                "Access denied: Null bytes are prohibited in file paths.".into(),
            ));
        }

        if raw_str.starts_with(r"\\?\") || raw_str.starts_with("//?/") {
            return Err(FastMcpError::ToolExecution(
                "Access denied: Verbatim UNC prefixes (\\\\?\\) are prohibited.".into(),
            ));
        }

        if raw_str.contains("//") || raw_str.contains(r"\\") {
            return Err(FastMcpError::ToolExecution(
                "Access denied: Consecutive path separators are prohibited.".into(),
            ));
        }

        #[cfg(not(windows))]
        if raw_str.len() >= 2
            && raw_str.as_bytes()[1] == b':'
            && raw_str.as_bytes()[0].is_ascii_alphabetic()
        {
            return Err(FastMcpError::ToolExecution(format!(
                "Access denied: Windows drive path '{}' is outside Unix root.",
                raw_str
            )));
        }

        for segment in raw_str.split(['/', '\\']) {
            if Self::is_reserved_device_name(segment) {
                return Err(FastMcpError::ToolExecution(format!(
                    "Access denied: '{}' is a reserved NTFS device name.",
                    segment
                )));
            }
            if Self::is_windows_short_name(segment) {
                return Err(FastMcpError::ToolExecution(format!(
                    "Access denied: 8.3 short name alias '{}' is prohibited.",
                    segment
                )));
            }
        }

        if self.shield_secrets && self.is_sensitive_path(requested) {
            return Err(FastMcpError::ToolExecution(format!(
                "SafeFS Secret Shield: Access denied to '{}'. Credential and sensitive files are protected.",
                requested.display()
            )));
        }

        let mut existing_prefix = PathBuf::new();
        let mut non_existing_tail = Vec::new();
        let mut in_non_existing = false;

        for comp in requested.components() {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => {
                    if in_non_existing {
                        return Err(FastMcpError::ToolExecution(
                            "Access denied: Parent directory traversal ('..') in non-existent path component is prohibited.".into(),
                        ));
                    }
                    existing_prefix.push("..");
                }
                Component::Prefix(p) => {
                    existing_prefix.push(p.as_os_str());
                }
                Component::RootDir => {
                    existing_prefix.push(Component::RootDir.as_os_str());
                }
                Component::Normal(c) => {
                    if in_non_existing {
                        non_existing_tail.push(c);
                    } else {
                        let candidate = existing_prefix.join(c);
                        if candidate.exists() {
                            existing_prefix = candidate;
                        } else {
                            in_non_existing = true;
                            non_existing_tail.push(c);
                        }
                    }
                }
            }
        }

        let base = if existing_prefix.as_os_str().is_empty() {
            dunce::canonicalize(Path::new(".")).map_err(|e| {
                FastMcpError::ToolExecution(format!("Failed to resolve base directory: {}", e))
            })?
        } else if existing_prefix.exists() {
            dunce::canonicalize(&existing_prefix).map_err(|e| {
                FastMcpError::ToolExecution(format!("Path canonicalization failed: {}", e))
            })?
        } else {
            return Err(FastMcpError::ToolExecution(format!(
                "Failed to resolve path prefix: '{}' does not exist",
                existing_prefix.display()
            )));
        };

        let mut target_to_check = base;
        for comp in non_existing_tail {
            target_to_check.push(comp);
        }

        let mut ancestor = target_to_check.as_path();
        loop {
            if ancestor.exists() {
                #[cfg(windows)]
                {
                    match win32::check_no_reparse_point_and_no_hardlink(ancestor) {
                        Ok((is_reparse, is_hardlink)) => {
                            if is_reparse {
                                return Err(FastMcpError::ToolExecution(format!(
                                    "SafeFS Violation: Reparse point/symlink detected at '{}'. Symlinks are prohibited.",
                                    ancestor.display()
                                )));
                            }
                            if is_hardlink {
                                return Err(FastMcpError::ToolExecution(format!(
                                    "SafeFS Violation: Hardlink detected at '{}'. Hardlinks are prohibited.",
                                    ancestor.display()
                                )));
                            }
                        }
                        Err(e) => {
                            return Err(FastMcpError::ToolExecution(format!(
                                "SafeFS Violation: Failed to inspect file attributes at '{}': {}",
                                ancestor.display(),
                                e
                            )));
                        }
                    }
                }
                #[cfg(unix)]
                {
                    if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
                        if meta.file_type().is_symlink() {
                            return Err(FastMcpError::ToolExecution(format!(
                                "SafeFS Violation: Symlink detected in path component '{}'. Symlinks are prohibited.",
                                ancestor.display()
                            )));
                        }
                        if (ancestor.is_file() || meta.is_file())
                            && (ancestor.is_hardlink() || meta.is_hardlink())
                        {
                            return Err(FastMcpError::ToolExecution(format!(
                                "SafeFS Violation: Hardlink detected at '{}'. Hardlinks are prohibited.",
                                ancestor.display()
                            )));
                        }
                    }
                }
            }
            match ancestor.parent() {
                Some(p) if !p.as_os_str().is_empty() && p != ancestor => ancestor = p,
                _ => break,
            }
        }

        if target_to_check.exists() {
            #[cfg(windows)]
            {
                let (is_reparse, is_hardlink) =
                    win32::check_no_reparse_point_and_no_hardlink(&target_to_check).map_err(
                        |e| FastMcpError::ToolExecution(format!("Failed to verify target: {}", e)),
                    )?;
                if is_reparse {
                    return Err(FastMcpError::ToolExecution(format!(
                        "SafeFS Violation: Symlink/reparse point detected at target '{}'. Symlinks are prohibited.",
                        target_to_check.display()
                    )));
                }
                if is_hardlink {
                    return Err(FastMcpError::ToolExecution(format!(
                        "SafeFS Violation: Hardlink detected at target '{}'. Hardlinks are prohibited.",
                        target_to_check.display()
                    )));
                }
            }
            #[cfg(unix)]
            {
                if let Ok(meta) = std::fs::symlink_metadata(&target_to_check) {
                    if meta.file_type().is_symlink() {
                        return Err(FastMcpError::ToolExecution(format!(
                            "SafeFS Violation: Symlink detected at target '{}'. Symlinks are prohibited.",
                            target_to_check.display()
                        )));
                    }
                    if (target_to_check.is_file() || meta.is_file())
                        && (target_to_check.is_hardlink() || meta.is_hardlink())
                    {
                        return Err(FastMcpError::ToolExecution(format!(
                            "SafeFS Violation: Hardlink detected at '{}'. Hardlinks are prohibited.",
                            target_to_check.display()
                        )));
                    }
                }
            }
            target_to_check = dunce::canonicalize(&target_to_check).map_err(|e| {
                FastMcpError::ToolExecution(format!("Canonicalization failed: {}", e))
            })?;
        }

        if self.shield_secrets && self.is_sensitive_path(&target_to_check) {
            return Err(FastMcpError::ToolExecution(format!(
                "SafeFS Secret Shield: Access denied to '{}'.",
                target_to_check.display()
            )));
        }

        if !self.enabled || self.allowed_roots.is_empty() {
            return Ok(target_to_check);
        }

        for root in &self.allowed_roots {
            if target_to_check
                .ancestors()
                .any(|a| path_components_equal(a, root))
            {
                return Ok(target_to_check);
            }
        }

        Err(FastMcpError::ToolExecution(format!(
            "SafeFS Security Violation: Target '{}' escapes authorized directories: {:?}",
            target_to_check.display(),
            self.allowed_roots
        )))
    }

    pub fn open_for_write_no_follow(&self, requested: &Path) -> Result<PathBuf, FastMcpError> {
        let safe_path = self.validate_path(requested)?;

        if let Some(parent) = safe_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    FastMcpError::ToolExecution(format!("Failed to create parent directory: {}", e))
                })?;
            }
        }

        #[cfg(windows)]
        {
            win32::open_new_or_verify_reparse_point(&safe_path).map_err(|e| {
                FastMcpError::ToolExecution(format!(
                    "SafeFS Violation: open_for_write_no_follow rejected '{}': {}",
                    safe_path.display(),
                    e
                ))
            })?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if safe_path.exists() {
                let meta = std::fs::symlink_metadata(&safe_path).map_err(|e| {
                    FastMcpError::ToolExecution(format!("Failed to inspect target metadata: {}", e))
                })?;
                if meta.file_type().is_symlink() {
                    return Err(FastMcpError::ToolExecution(format!(
                        "SafeFS Violation: Symlink detected at target '{}'. Overwrite prohibited.",
                        safe_path.display()
                    )));
                }
                if meta.is_hardlink() {
                    return Err(FastMcpError::ToolExecution(format!(
                        "SafeFS Violation: Hardlink detected at target '{}'. Overwrite prohibited.",
                        safe_path.display()
                    )));
                }
                let _ = std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&safe_path)
                    .map_err(|e| {
                        FastMcpError::ToolExecution(format!(
                            "Symlink traversal blocked by O_NOFOLLOW: {}",
                            e
                        ))
                    })?;
            } else {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&safe_path)
                    .map_err(|e| {
                        FastMcpError::ToolExecution(format!(
                            "Failed to create file with O_NOFOLLOW: {}",
                            e
                        ))
                    })?;
            }
        }

        Ok(safe_path)
    }
}
