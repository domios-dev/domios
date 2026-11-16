use super::*;

#[test]
fn unavailable_metadata_never_defaults_on_or_replaces_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    State::default().save(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::NotADirectory,
        std::io::ErrorKind::Other,
    ] {
        let result = State::read_with_metadata(&path, |_| Err(std::io::Error::from(kind)));
        assert_eq!(
            result.err().as_deref(),
            Some("analytics settings unavailable")
        );
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }
    let retained = State::read(&path).unwrap();
    assert_eq!(retained.consent, Consent::Off);
    assert!(retained.device_id.is_none());
    let fresh = State::read(&dir.path().join("absent.json")).unwrap();
    assert_eq!(fresh.consent, Consent::On);
    assert!(fresh.device_id.is_some());
}

#[test]
fn retired_feature_days_preserves_consent_and_device_then_disappears_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let device = Uuid::parse_str("12345678-1234-4234-8234-123456789abc").unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "consent":"on", "device_id":device,
            "feature_days":{"memory_saved":"2026-10-13"}
        }))
        .unwrap(),
    )
    .unwrap();
    let state = State::read(&path).unwrap();
    assert_eq!(state.consent, Consent::On);
    assert_eq!(state.device_id, Some(device));
    state.save(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["consent"], "on");
    assert_eq!(saved["device_id"], "12345678-1234-4234-8234-123456789abc");
    assert!(saved.get("feature_days").is_none());
}
