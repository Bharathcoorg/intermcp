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
