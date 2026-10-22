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
        root.path(),
    )
    .unwrap();
    assert_eq!(std::fs::read(&staged.path).unwrap(), bytes);
    assert_eq!(
        staged.command(),
        format!("sudo dpkg -i -- '{}'", staged.path.display())
    );
    assert_eq!(staged.response()["status"], "elevation_required");
    assert!(!staged.command().contains("quit"));
    let before = std::fs::read_dir(root.path()).unwrap().count();
    assert!(verify_and_stage(
        b"tampered package",
        &encode(signature.as_bytes()),
        &encode(key.as_bytes()),
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
