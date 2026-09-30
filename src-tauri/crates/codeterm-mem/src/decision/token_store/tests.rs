use super::*;

const TOKEN: &str = "local-test-secret";

fn store() -> (tempfile::TempDir, TokenStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path().join("tokens"));
    (dir, store)
}

#[cfg(unix)]
#[test]
fn token_store_set_creates_0600_file() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, store) = store();
    store.set("typesafe-jev", TOKEN).unwrap();
    assert_eq!(
        fs::metadata(store.path("typesafe-jev"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&store.base_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(store.load("typesafe-jev").unwrap().unwrap().expose() == TOKEN);
}

#[test]
fn token_store_clear_removes_file() {
    let (_dir, store) = store();
    assert!(!store.clear("typesafe-jev").unwrap());
    store.set("typesafe-jev", TOKEN).unwrap();
    assert!(store.clear("typesafe-jev").unwrap());
    assert!(!store.clear("typesafe-jev").unwrap());
}

#[test]
fn token_store_status_reports_presence_without_value() {
    let (_dir, store) = store();
    assert!(!store.status("typesafe-jev").unwrap().present);
    store.set("typesafe-jev", TOKEN).unwrap();
    let status = store.status("typesafe-jev").unwrap();
    assert!(status.present);
    assert!(!format!("{status:?}").contains(TOKEN));
    #[cfg(unix)]
    assert_eq!(status.mode_ok, Some(true));
    #[cfg(not(unix))]
    assert_eq!(status.mode_ok, None);
}

#[test]
fn token_store_id_cannot_escape_base_dir() {
    let (_dir, store) = store();
    let path = store.path("../x");
    assert_eq!(path.parent(), Some(store.base_dir.as_path()));
    store.set("../x", TOKEN).unwrap();
    assert!(path.exists());
}

#[test]
fn token_store_rejects_empty_token() {
    let (_dir, store) = store();
    assert_eq!(
        store.set("typesafe-jev", "  \n ").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(!store.path("typesafe-jev").exists());
}

#[test]
fn token_store_debug_and_errors_are_redacted() {
    let (_dir, store) = store();
    store.set("typesafe-jev", TOKEN).unwrap();
    let token = store.load("typesafe-jev").unwrap().unwrap();
    assert_eq!(format!("{token:?}"), "ApiToken(<redacted>)");
    assert!(!format!("{store:?}").contains(TOKEN));
    assert!(!format!("{:?}", store.status("typesafe-jev").unwrap()).contains(TOKEN));
    let errors = [
        super::super::DecisionError::Unavailable("token_missing".into()),
        super::super::DecisionError::Deadline,
        super::super::DecisionError::Http(401),
        super::super::DecisionError::Transport("request_failed".into()),
        super::super::DecisionError::Parse("invalid_answer".into()),
    ];
    for error in errors {
        assert!(!format!("{error:?}").contains(TOKEN));
        assert!(!error.to_string().contains(TOKEN));
    }
    fs::write(store.path("typesafe-jev"), TOKEN).unwrap();
    let error = store.load("typesafe-jev").unwrap_err();
    assert!(!error.to_string().contains(TOKEN));
}
