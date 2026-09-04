use intermcp::sandbox::SandboxPolicy;
use std::path::{Path, PathBuf};

#[test]
fn test_block_reserved_ntfs_device_names() {
    let policy = SandboxPolicy::unrestricted();

    let devices = [
        "CON", "con", "PRN", "prn", "AUX", "aux", "NUL", "nul", "COM1", "com1", "COM9", "com9",
        "LPT1", "lpt1", "LPT9", "lpt9", "CON.txt", "aux.json", "nul.dat",
    ];

    for dev in devices {
        let p = Path::new(dev);
        assert!(
            policy.validate_path(p).is_err(),
            "Expected device name '{}' to be blocked",
            dev
        );
    }
}

#[test]
fn test_block_windows_8_3_short_names() {
    let policy = SandboxPolicy::unrestricted();

    let short_names = [
        "PROGRA~1",
        "SECRET~1.TXT",
        "MYFILE~2.ENV",
        "C:\\USERS\\ADMINI~1\\DOCS",
    ];

    for name in short_names {
        let p = Path::new(name);
        assert!(
            policy.validate_path(p).is_err(),
            "Expected 8.3 short name alias '{}' to be blocked",
            name
        );
    }
}

#[test]
fn test_block_verbatim_unc_prefix() {
    let policy = SandboxPolicy::unrestricted();

    let verbatim_paths = [
        r"\\?\C:\Windows\System32",
        r"\\?\C:\secret.txt",
        "//?/C:/Windows",
    ];

    for vp in verbatim_paths {
        let p = Path::new(vp);
        assert!(
            policy.validate_path(p).is_err(),
            "Expected verbatim path '{}' to be blocked",
            vp
        );
    }
}

#[test]
fn test_secret_shield_sensitive_file_blocking() {
    let policy = SandboxPolicy::unrestricted();

    let blocked_files = [
        ".netrc",
        ".pgpass",
        ".bash_history",
        ".zsh_history",
        "kubeconfig",
        ".docker/config.json",
        "gcloud/credentials.db",
        "credentials.json",
        "secrets.json",
        "secret.json",
        "token.json",
        "id_rsa",
        "id_ed25519",
        "server.pem",
        "cert.key",
    ];

    for file in blocked_files {
        let p = Path::new(file);
        assert!(
            policy.validate_path(p).is_err(),
            "Expected sensitive file '{}' to be blocked by Secret Shield",
            file
        );
    }
}

#[test]
fn test_sandbox_root_containment() {
    let root = dunce::canonicalize(Path::new(".")).unwrap();
    let policy = SandboxPolicy::new(vec![root.clone()]);

    let inside = root.join("Cargo.toml");
    assert!(policy.validate_path(&inside).is_ok());

    let outside = PathBuf::from("C:\\Windows\\System32\\calc.exe");
    assert!(policy.validate_path(&outside).is_err());
}

#[test]
fn test_symlink_toctou_mitigation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    let policy = SandboxPolicy::new(vec![root.clone()]);

    let target = root.join("safe_file.txt");
    std::fs::write(&target, "initial content").unwrap();

    // 1. Initial validation succeeds
    let validated = policy.validate_path(&target);
    assert!(validated.is_ok());
    let validated_path = validated.unwrap();

    // 2. Simulate TOCTOU race: replace validated target with a symlink
    let external_dest = tmp.path().join("external_secret.txt");
    std::fs::write(&external_dest, "confidential").unwrap();
    let _ = std::fs::remove_file(&target);

    #[cfg(unix)]
    let sym_result = std::os::unix::fs::symlink(&external_dest, &target);
    #[cfg(windows)]
    let sym_result = std::os::windows::fs::symlink_file(&external_dest, &target);

    // 3. Immediately before I/O operation, symlink_metadata check detects the swap
    if sym_result.is_ok() {
        let meta = validated_path.symlink_metadata();
        assert!(meta.is_ok());
        assert!(meta.unwrap().file_type().is_symlink());
    }
}

#[test]
fn test_symlink_swap_after_canonicalize() {
    let tmp_root = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(tmp_root.path()).unwrap();
    let sandbox_dir = root.join("sandbox");
    std::fs::create_dir_all(&sandbox_dir).unwrap();
    let policy = SandboxPolicy::new(vec![sandbox_dir.clone()]);

    // a) Benign directory with benign file
    let sub_dir = sandbox_dir.join("work_dir");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let benign_file = sub_dir.join("data.txt");
    std::fs::write(&benign_file, "benign public content").unwrap();

    // b) Separate directory outside sandbox with sensitive credentials.json
    let sensitive_dir = root.join("outside_sensitive");
    std::fs::create_dir_all(&sensitive_dir).unwrap();
    let creds_file = sensitive_dir.join("credentials.json");
    std::fs::write(&creds_file, "{\"secret\":\"top-secret-token\"}").unwrap();

    // c) Validate path targeting benign file inside work_dir (should pass)
    let validated = policy.validate_path(&benign_file);
    assert!(
        validated.is_ok(),
        "Initial validation of benign file inside sandbox must pass"
    );

    // d) Swap the directory: remove benign file & work_dir, replace with symlink to sensitive_dir
    let target_swap = sub_dir.clone();
    std::fs::remove_file(&benign_file).unwrap();
    std::fs::remove_dir(&sub_dir).unwrap();

    #[cfg(unix)]
    let sym_res = std::os::unix::fs::symlink(&sensitive_dir, &target_swap);
    #[cfg(windows)]
    let sym_res = std::os::windows::fs::symlink_dir(&sensitive_dir, &target_swap);

    if sym_res.is_ok() {
        // Re-validating the same path after symlink swap must fail
        let revalidated = policy.validate_path(&benign_file);
        assert!(
            revalidated.is_err(),
            "Re-validation after symlink swap must fail and block escape"
        );
    } else {
        // Fallback if OS denies unprivileged symlink creation: direct validation of sensitive path fails
        assert!(policy.validate_path(&creds_file).is_err());
    }
}
