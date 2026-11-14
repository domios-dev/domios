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
