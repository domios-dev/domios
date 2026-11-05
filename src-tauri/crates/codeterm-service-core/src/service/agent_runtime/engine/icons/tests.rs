use super::*;

#[test]
fn identity_lookup_requires_exact_unambiguous_registry_evidence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("cache")).unwrap();
    let path = root.path().join("cache/registry-v1.json");
    std::fs::write(&path, r#"{"version":"1.0.0","agents":[{"id":"fixture","name":"Fixture Agent","icon":"https://example.org/a.svg"}]}"#).unwrap();
    assert_eq!(
        registry_url(root.path(), "Fixture Agent").as_deref(),
        Some("https://example.org/a.svg")
    );
    assert!(registry_url(root.path(), "Fixture").is_none());
    std::fs::write(path, r#"{"version":"1.0.0","agents":[{"id":"a","name":"Same","icon":"https://example.org/a.svg"},{"id":"b","name":"Same","icon":"https://example.org/b.svg"}]}"#).unwrap();
    assert!(registry_url(root.path(), "Same").is_none());
}
