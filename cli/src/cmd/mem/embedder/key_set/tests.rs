use super::*;

#[test]
fn mem_embedder_key_set_rejects_terminal_empty_oversized_and_invalid_input() {
    let dir = tempfile::tempdir().unwrap();
    for (provider, input, terminal) in [
        ("openai", "fake-key".to_string(), true),
        ("openai", " \r\n".to_string(), false),
        ("openai", "x".repeat(65537), false),
        ("../outside", "fake-key".to_string(), false),
        ("", "fake-key".to_string(), false),
    ] {
        let error = set(dir.path(), provider, input.as_bytes(), terminal).unwrap_err();
        assert!(!error.contains("fake-key"));
        assert!(!dir.path().join("secrets").exists());
    }
    assert!(set(dir.path(), "openai", &[0xff][..], false).is_err());
}

#[cfg(unix)]
#[test]
fn mem_embedder_key_set_refuses_a_secrets_symlink_outside_the_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("secrets")).unwrap();
    assert!(set(dir.path(), "openai", b"fake-key".as_slice(), false).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
