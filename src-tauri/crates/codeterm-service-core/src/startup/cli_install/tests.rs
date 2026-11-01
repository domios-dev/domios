use super::file_sha256;
use std::io::Write;

/// Stands in for the registry writer. With this wired in there is no code
/// path from a test to `HKCU\Environment` at all.
#[derive(Default)]
struct RecordingUserPath {
    added: std::sync::Mutex<Vec<std::path::PathBuf>>,
}

impl RecordingUserPath {
    fn added(&self) -> Vec<std::path::PathBuf> {
        self.added.lock().unwrap().clone()
    }
}

impl super::UserPathWriter for RecordingUserPath {
    fn add_dir(&self, dir: &std::path::Path) {
        self.added.lock().unwrap().push(dir.to_path_buf());
    }
}

#[test]
fn the_staged_plain_cli_name_wins_over_a_leftover_triple_named_copy() {
    let names = super::bundled_cli_resource_names();
    assert_eq!(names[0], format!("cli/{}", super::cli_bin_name()));
}

#[test]
fn startup_bundled_cli_never_overwrites_dev_cli() {
    assert!(!super::should_install_bundled_cli(true));
    assert!(super::should_install_bundled_cli(false));
}

#[cfg(windows)]
fn write_pe(path: &std::path::Path, machine: u16) {
    let mut bytes = vec![0u8; 0x86];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&(0x80u32).to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

#[cfg(windows)]
#[test]
fn bundled_cli_rejects_foreign_windows_architecture() {
    let dir = tempfile::tempdir().unwrap();
    let x64 = dir.path().join("x64.exe");
    let arm64 = dir.path().join("arm64.exe");
    write_pe(&x64, 0x8664);
    write_pe(&arm64, 0xaa64);

    #[cfg(target_arch = "x86_64")]
    {
        assert!(super::bundled_cli_matches_host(&x64));
        assert!(!super::bundled_cli_matches_host(&arm64));
    }
    #[cfg(target_arch = "aarch64")]
    {
        assert!(!super::bundled_cli_matches_host(&x64));
        assert!(super::bundled_cli_matches_host(&arm64));
    }
}

#[test]
fn hash_distinguishes_same_size_different_content() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.bin");
    let b = dir.path().join("b.bin");
    // Two files of identical length but different content — exactly the v1.1.5/1.1.6 failure mode that defeated the old size-only skip.
    std::fs::File::create(&a)
        .unwrap()
        .write_all(&[0u8; 64])
        .unwrap();
    std::fs::File::create(&b)
        .unwrap()
        .write_all(&[1u8; 64])
        .unwrap();
    let ha = file_sha256(&a).expect("hash a");
    let hb = file_sha256(&b).expect("hash b");
    assert_eq!(
        std::fs::metadata(&a).unwrap().len(),
        std::fs::metadata(&b).unwrap().len()
    );
    assert_ne!(
        ha, hb,
        "same-size files with different content must hash differently"
    );
}

#[test]
fn hash_is_deterministic_for_same_content() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("c.bin");
    std::fs::File::create(&p)
        .unwrap()
        .write_all(b"codeterm-cli-fixture")
        .unwrap();
    assert_eq!(file_sha256(&p).unwrap(), file_sha256(&p).unwrap());
}

#[test]
fn refresh_installs_then_skips_then_reinstalls_on_change() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let data_dir = home.path().join(".domios");
    let srcdir = tempfile::tempdir().unwrap();
    let src = srcdir.path().join("codeterm");
    let user_path = RecordingUserPath::default();
    std::fs::File::create(&src)
        .unwrap()
        .write_all(b"v1.5.6-binary")
        .unwrap();

    // First refresh installs the CLI.
    assert!(super::refresh_installed_cli_with(&src, home.path(), &data_dir, &user_path).unwrap());
    let dest = data_dir.join("bin").join(super::cli_bin_name());
    assert!(dest.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"v1.5.6-binary");
    let alias = data_dir.join("bin").join(if cfg!(windows) {
        "domios.exe"
    } else {
        "domios"
    });
    assert_eq!(
        std::fs::read(&alias).unwrap(),
        std::fs::read(&dest).unwrap()
    );
    std::fs::remove_file(&alias).unwrap();

    // Same content → up-to-date, no reinstall.
    assert!(!super::refresh_installed_cli_with(&src, home.path(), &data_dir, &user_path).unwrap());

    assert_eq!(
        std::fs::read(&alias).unwrap(),
        std::fs::read(&dest).unwrap()
    );

    // New release with same name but different content → reinstall.
    std::fs::File::create(&src)
        .unwrap()
        .write_all(b"v1.5.7-binary-with-report-to")
        .unwrap();
    assert!(super::refresh_installed_cli_with(&src, home.path(), &data_dir, &user_path).unwrap());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"v1.5.7-binary-with-report-to"
    );

    assert_eq!(
        std::fs::read(&alias).unwrap(),
        std::fs::read(&dest).unwrap()
    );

    // The registry is never reached: every prod-dest refresh hands the bin
    // directory to the seam and nothing else.
    assert_eq!(
        user_path.added(),
        vec![
            data_dir.join("bin"),
            data_dir.join("bin"),
            data_dir.join("bin")
        ],
        "each refresh must hand exactly the installed bin directory to the seam"
    );
}

#[test]
fn refresh_dev_instance_installs_into_data_dir_not_prod() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let data_dir = home.path().join(".domios-dev-phase-c-linux");
    let srcdir = tempfile::tempdir().unwrap();
    let src = srcdir.path().join("codeterm");
    std::fs::File::create(&src)
        .unwrap()
        .write_all(b"dev-instance-binary")
        .unwrap();

    let user_path = RecordingUserPath::default();
    assert!(super::refresh_installed_cli_with(&src, home.path(), &data_dir, &user_path).unwrap());

    let dev_dest = data_dir.join("bin").join(super::cli_bin_name());
    let prod_dest = home
        .path()
        .join(".domios")
        .join("bin")
        .join(super::cli_bin_name());
    assert_eq!(std::fs::read(&dev_dest).unwrap(), b"dev-instance-binary");
    let primary = data_dir
        .join("bin")
        .join(format!("domios{}", std::env::consts::EXE_SUFFIX));
    assert_eq!(std::fs::read(&primary).unwrap(), b"dev-instance-binary");
    assert!(
        !prod_dest.exists(),
        "dev/named instance CLI refresh must not write prod bin"
    );
    // Companion to the seam assertion above: a non-prod destination reaches
    // the seam with nothing, so a writer that records unconditionally fails.
    assert!(
        user_path.added().is_empty(),
        "a dev/named instance must never touch the persistent user PATH"
    );
}

#[test]
fn hash_returns_none_on_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.bin");
    assert!(file_sha256(&missing).is_none());
}

#[test]
fn resource_hint_is_private_and_refreshed_for_the_installed_cli() {
    use codeterm_types::server_bootstrap::{CliResourceHint, CLI_RESOURCE_HINT_FILE};
    let home = tempfile::tempdir().unwrap();
    let data = home.path().join("data");
    let resources = home.path().join("resources");
    std::fs::create_dir_all(resources.join("cli")).unwrap();
    let source = resources.join("cli/codeterm");
    let writer = RecordingUserPath::default();
    std::fs::write(&source, b"first").unwrap();
    super::refresh_installed_cli_with(&source, home.path(), &data, &writer).unwrap();
    let hint_path = data.join("bin").join(CLI_RESOURCE_HINT_FILE);
    let first: CliResourceHint =
        serde_json::from_slice(&std::fs::read(&hint_path).unwrap()).unwrap();
    assert_eq!(first.resources, resources.canonicalize().unwrap());
    assert_eq!(first.cli_sha256, file_sha256(&source).unwrap());
    assert!(codeterm_util::private_file::owner_only(&hint_path).unwrap());
    std::fs::write(&source, b"second").unwrap();
    super::refresh_installed_cli_with(&source, home.path(), &data, &writer).unwrap();
    let second: CliResourceHint =
        serde_json::from_slice(&std::fs::read(&hint_path).unwrap()).unwrap();
    assert_eq!(second.resources, first.resources);
    assert_ne!(second.cli_sha256, first.cli_sha256);
    assert_eq!(second.cli_sha256, file_sha256(&source).unwrap());
    std::fs::remove_file(&hint_path).unwrap();
    assert!(!super::refresh_installed_cli_with(&source, home.path(), &data, &writer).unwrap());
    assert!(hint_path.is_file());
}
