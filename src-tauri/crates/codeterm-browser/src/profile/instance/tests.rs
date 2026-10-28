use super::*;
use serde_json::json;
#[test]
fn named_sessions_bind_exact_origins_and_lifecycle_without_adopting_user_profiles() {
    let root = tempfile::tempdir().unwrap();
    let layout = ProfileLayout::new(root.path().to_path_buf());
    layout
        .prepare_instance("qa", "http://localhost:7686", false)
        .unwrap();
    layout
        .prepare_instance("qa", "http://localhost:7686/", false)
        .unwrap();
    assert!(layout
        .prepare_instance("qa", "http://localhost:7687", false)
        .unwrap_err()
        .starts_with("origin_mismatch:"));
    assert!(layout
        .prepare_instance("qa", "http://localhost:7686", true)
        .is_err());
    layout.create_named("user").unwrap();
    assert!(layout
        .prepare_instance("user", "http://localhost:7686", false)
        .unwrap_err()
        .starts_with("unbound_profile:"));
    assert!(layout
        .prepare_instance("NUL", "http://localhost:7686", false)
        .is_err());
}
#[test]
fn instance_origins_and_expected_builds_refuse_credential_or_identity_skew() {
    for raw in [
        "http://user:pass@localhost:7686",
        "http://localhost:7686/?token=secret",
        "http://localhost:7686/#token=secret",
        "http://localhost:7686/web",
        "file:///tmp",
    ] {
        assert!(normalize_origin(raw).is_err());
    }
    assert_eq!(
        normalize_origin("http://LOCALHOST:7686/").unwrap(),
        "http://localhost:7686"
    );
    assert!(verify_build(&json!({"build_sha":"abc"}), "abc").is_ok());
    assert!(verify_build(&json!({"build_sha":"other"}), "abc").is_err());
    assert!(verify_build(&json!({}), "abc").is_err());
}

#[test]
fn dev_identity_requires_exact_name_port_and_development_identifier() {
    let health =
        json!({"instance":{"name":"qa","api_port":"7790","identifier":"com.domios.dev.qa"}});
    verify_dev_identity(&health, "qa", 7790).unwrap();
    for (field, value) in [
        ("name", "other"),
        ("api_port", "7685"),
        ("identifier", "com.domios.app"),
    ] {
        let mut wrong = health.clone();
        wrong["instance"][field] = json!(value);
        assert!(verify_dev_identity(&wrong, "qa", 7790).is_err());
    }
    assert!(verify_dev_identity(&health, "qa", 7685).is_err());
    assert!(verify_dev_identity(&json!({}), "qa", 7790).is_err());
}
