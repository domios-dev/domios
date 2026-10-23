use super::*;

#[test]
fn plugin_updates_never_discover_a_different_dev_instance() {
    assert!(scan_production(None, None));
    assert!(scan_production(Some("7685"), None));
    for port in ["7686", "17686"] {
        assert!(!scan_production(Some(port), None));
    }
    assert!(!scan_production(None, Some("http://127.0.0.1:17686")));
}

#[test]
fn only_proven_absence_permits_a_local_plugin_update() {
    assert!(absent(ErrorKind::NotFound));
    assert!(absent(ErrorKind::ConnectionRefused));
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::TimedOut,
        ErrorKind::WouldBlock,
        ErrorKind::ConnectionReset,
        ErrorKind::InvalidInput,
        ErrorKind::UnexpectedEof,
    ] {
        assert!(!absent(kind), "{kind:?}");
    }
}
