use super::*;

#[test]
fn shared_materialization_validates_before_atomic_swap() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let cache = root.path().join("cache");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("payload"), b"first").unwrap();
    materialize_channel("fixture", source.to_str().unwrap(), &cache).unwrap();
    std::fs::write(source.join("payload"), b"second").unwrap();
    let result = refresh_materialized_channel("fixture", source.to_str().unwrap(), &cache, |_| {
        Err("refused".into())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(cache.join("payload")).unwrap(), b"first");
    refresh_materialized_channel("fixture", source.to_str().unwrap(), &cache, |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(cache.join("payload")).unwrap(), b"second");
    assert!(materialize_channel("../escape", source.to_str().unwrap(), &cache).is_err());
    for name in ["nul", "con", "com1", "lpt1.log"] {
        assert!(validate_channel_name(name).is_err());
    }
}

#[test]
fn channel_paths_are_classified_by_spelling_and_a_missing_folder_is_typed() {
    for local in [
        "C:/no/such",
        r"C:\no\such",
        "./relative",
        "/absolute",
        r"\\server\share",
        "file:///x",
        "relative/dir",
    ] {
        assert_eq!(channel_kind(local), ChannelKind::Local, "{local}");
    }
    for remote in [
        "https://example.invalid/r.git",
        "git://127.0.0.1/channel",
        "git@github.com:owner/repo.git",
        "github.com:owner/repo",
    ] {
        assert_ne!(channel_kind(remote), ChannelKind::Local, "{remote}");
    }
    let root = tempfile::tempdir().unwrap();
    let missing = root
        .path()
        .join("absent")
        .to_string_lossy()
        .replace('\\', "/");
    let failure = fetch_channel("fixture", &missing, &root.path().join("cache")).unwrap_err();
    assert_eq!(failure.kind, ChannelFailureKind::FolderNotFound);
}
