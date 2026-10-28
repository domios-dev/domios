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
