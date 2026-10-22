use super::*;

#[test]
fn only_owned_refused_or_missing_sockets_are_stale() {
    use io::ErrorKind::*;
    for owned in [false, true] {
        assert!(!stale_socket(owned, Ok(())));
        for kind in [
            ConnectionRefused,
            NotFound,
            PermissionDenied,
            WouldBlock,
            TimedOut,
            Other,
        ] {
            assert_eq!(
                stale_socket(owned, Err(kind)),
                owned && matches!(kind, ConnectionRefused | NotFound)
            );
        }
    }
}

#[test]
fn bind_failure_preserves_kind_and_names_target() {
    let error = bind_error("127.0.0.1:9876", io::Error::from(io::ErrorKind::AddrInUse));
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    assert!(error.to_string().contains("127.0.0.1:9876"));
}

#[cfg(unix)]
#[test]
fn stale_unix_socket_is_reclaimed_but_live_socket_and_regular_file_survive() {
    use std::os::unix::net::UnixListener;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("endpoint.sock");
    let listener = UnixListener::bind(&path).unwrap();
    assert!(prepare_path(&path).is_err());
    assert!(path.exists());
    drop(listener);
    prepare_path(&path).unwrap();
    let rebound = UnixListener::bind(&path).unwrap();
    drop(rebound);
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"keep").unwrap();
    assert!(prepare_path(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep");
}
