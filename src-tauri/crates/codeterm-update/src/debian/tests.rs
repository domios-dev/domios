use super::*;

#[test]
fn only_signature_verified_bytes_are_staged_and_exposed_to_dpkg() {
    let bytes =
        include_bytes!("../../../../../scripts/frontend-release/fixtures/frontend-manifest.json");
    let signature = include_str!(
        "../../../../../scripts/frontend-release/fixtures/frontend-manifest.json.minisig"
    );
    let key =
        "untrusted comment: fixture\nRWRmcm9udGVuZNEdQEUZEh9CtfmYUwGDKXT9+qbRXfdx90/9a+hv/8OA\n";
    let encode = |value: &[u8]| base64::engine::general_purpose::STANDARD.encode(value);
    let root = tempfile::tempdir().unwrap();
    let staged = verify_and_stage(
        bytes,
        &encode(signature.as_bytes()),
        &encode(key.as_bytes()),
        "1.17.2",
        root.path(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(staged.path.parent().unwrap()).unwrap();
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.mode() & 0o777, 0o700);
    }
    assert_eq!(std::fs::read(&staged.path).unwrap(), bytes);
    assert_eq!(
        staged.command(),
        format!("sudo dpkg -i -- '{}'", staged.path.display())
    );
    assert_eq!(staged.response()["status"], "elevation_required");
    assert!(!staged.command().contains("quit"));
    let original_path = staged.path.clone();
    let original_sha = staged.sha256.clone();
    assert!(staging::reuse(
        root.path(),
        "1.17.2",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes())
    )
    .is_err());
    drop(staged);
    let reused = staging::reuse(
        root.path(),
        "1.17.2",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(reused.path, original_path);
    assert_eq!(reused.sha256, original_sha);
    drop(reused);
    assert!(staging::reuse(
        root.path(),
        "1.17.3",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes())
    )
    .unwrap()
    .is_none());
    std::fs::write(&original_path, b"tampered").unwrap();
    assert!(staging::reuse(
        root.path(),
        "1.17.2",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes())
    )
    .unwrap()
    .is_none());
    let before = std::fs::read_dir(root.path()).unwrap().count();
    assert!(verify_and_stage(
        b"tampered package",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes()),
        "1.17.2",
        root.path()
    )
    .is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), before);
}

#[test]
fn staged_path_is_shell_quoted_and_quit_is_a_separate_step() {
    let command = staged_command(Path::new("/tmp/a'b.deb")).unwrap();
    assert_eq!(command, "sudo dpkg -i -- '/tmp/a'\\''b.deb'");
    assert!(!command.contains("&&"));
}
