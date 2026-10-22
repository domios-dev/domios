use super::*;

#[test]
fn public_build_key_is_overridden_by_local_secret_store() {
    let store: SecretReader = std::sync::Arc::new(|name| {
        assert_eq!(name, "domios.amplitude.prod.api_key");
        Ok("local-key".into())
    });
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Prod, &store).as_deref(),
        Some("local-key")
    );
    let missing: SecretReader = std::sync::Arc::new(|_| Err("missing".into()));
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Prod, &missing).as_deref(),
        Some("public-build-key")
    );
    assert_eq!(resolve_key(None, Project::Prod, &missing), None);
    let blank: SecretReader = std::sync::Arc::new(|_| Ok(" ".into()));
    assert_eq!(
        resolve_key(Some("public-build-key"), Project::Test, &blank).as_deref(),
        Some("public-build-key")
    );
    assert_eq!(resolve_key(Some(" "), Project::Test, &blank), None);
}
