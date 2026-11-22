use super::*;

#[test]
fn restored_profile_icon_reaches_snapshot_only_for_current_epoch() {
    let mut state = RuntimeState::unloaded(Some("restored-thread".into()));
    state.epoch = 7;
    state.icon_svg = Some(String::new());
    let icon = "<svg><path d=\"M1 1\"/></svg>";
    assert!(apply_icon(&mut state, 7, Some(icon.into())));
    assert_eq!(state.snapshot().icon_svg.as_deref(), Some(icon));
    assert!(!apply_icon(&mut state, 6, Some("<svg/>".into())));
    assert!(!apply_icon(&mut state, 7, Some("  ".into())));
    assert!(!apply_icon(&mut state, 7, None));
    assert_eq!(state.snapshot().icon_svg.as_deref(), Some(icon));
}

#[test]
fn identity_lookup_requires_exact_unambiguous_registry_evidence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("cache")).unwrap();
    let path = root.path().join("cache/registry-v1.json");
    std::fs::write(&path, r#"{"version":"1.0.0","agents":[{"id":"fixture","name":"Fixture Agent","icon":"https://example.org/a.svg"}]}"#).unwrap();
    assert_eq!(
        registry_url(root.path(), Some("Fixture Agent"), None).as_deref(),
        Some("https://example.org/a.svg")
    );
    assert!(registry_url(root.path(), Some("Fixture"), None).is_none());
    assert_eq!(
        registry_url(root.path(), Some("Different runtime name"), Some("fixture")).as_deref(),
        Some("https://example.org/a.svg")
    );
    assert_eq!(
        registry_url(root.path(), None, Some("fixture")).as_deref(),
        Some("https://example.org/a.svg")
    );
    assert!(registry_url(root.path(), Some("Fixture Agent"), Some("missing")).is_none());
    assert!(registry_url(root.path(), None, None).is_none());
    std::fs::write(path, r#"{"version":"1.0.0","agents":[{"id":"a","name":"Same","icon":"https://example.org/a.svg"},{"id":"b","name":"Same","icon":"https://example.org/b.svg"}]}"#).unwrap();
    assert!(registry_url(root.path(), Some("Same"), None).is_none());
    assert_eq!(
        registry_url(root.path(), Some("Same"), Some("b")).as_deref(),
        Some("https://example.org/b.svg")
    );
}

#[test]
fn profile_icon_resolution_prefers_refreshed_registry_url_and_preserves_local_overrides() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("cache")).unwrap();
    std::fs::write(
        root.path().join("cache/registry-v1.json"),
        r#"{"version":"1.0.0","agents":[{"id":"fixture","name":"Fixture","icon":"https://cdn.example.test/v2.svg"},{"id":"bad","name":"Bad","icon":"http://cdn.example.test/bad.svg"}]}"#,
    )
    .unwrap();

    let registry_profile: RuntimeLaunchProfile = serde_json::from_value(serde_json::json!({
        "binary":"fixture-bin","args":[],"ownerConfigured":true,
        "registryId":"fixture","iconUrl":"https://cdn.example.test/v1.svg"
    }))
    .unwrap();
    assert_eq!(
        resolve_profile_icon_url(root.path(), &registry_profile).as_deref(),
        Some("https://cdn.example.test/v2.svg")
    );

    let local_override: RuntimeLaunchProfile = serde_json::from_value(serde_json::json!({
        "binary":"fixture-bin","args":[],"ownerConfigured":true,
        "icon":"bundled-color.svg","registryId":"fixture","iconUrl":"https://cdn.example.test/v1.svg"
    }))
    .unwrap();
    assert!(resolve_profile_icon_url(root.path(), &local_override).is_none());

    let insecure_refresh: RuntimeLaunchProfile = serde_json::from_value(serde_json::json!({
        "binary":"fixture-bin","args":[],"ownerConfigured":true,
        "registryId":"bad","iconUrl":"https://cdn.example.test/safe-fallback.svg"
    }))
    .unwrap();
    assert_eq!(
        resolve_profile_icon_url(root.path(), &insecure_refresh).as_deref(),
        Some("https://cdn.example.test/safe-fallback.svg")
    );
}
