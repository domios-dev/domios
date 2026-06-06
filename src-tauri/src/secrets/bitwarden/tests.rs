use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;

use crate::secrets::keychain::{Keychain, KeychainError};
use crate::secrets::{
    ListFilter, SaveRequest, SecretError, SecretStore, SecretString, StoreStatus, UnlockCredentials,
};

use super::exec::{BwExec, BwResponse, BwRunCtx, NoopBwExec};
use super::store::{is_valid_http_url, unwrap_data_envelope, BitwardenStore};
use super::subprocess::{fallback_bw_paths, first_executable, is_executable_file};
use super::{
    map_bw_message, BW_KEYRING_API_CLIENT_ID, BW_KEYRING_API_CLIENT_SECRET, BW_KEYRING_SESSION,
};

/// Scripted `BwExec`: each invocation pops the next response off a
/// queue. Lets tests rehearse the exact sequence of `bw` calls a real
/// flow would produce, including failure modes.
struct ScriptedExec {
    responses: Mutex<Vec<BwResponse>>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl ScriptedExec {
    fn new(responses: Vec<BwResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn call_args(&self) -> Vec<Vec<String>> {
        self.calls.lock().clone()
    }
}

#[async_trait]
impl BwExec for ScriptedExec {
    async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
        let captured: Vec<String> = ctx.args.iter().map(|s| s.to_string()).collect();
        self.calls.lock().push(captured);
        self.responses
            .lock()
            .pop()
            .ok_or_else(|| "no scripted response left".to_string())
            .map(|mut r| {
                // Default success=true if data is non-null and field unset.
                if !r.success && r.message.is_none() && !r.data.is_null() {
                    r.success = true;
                }
                r
            })
    }
}

fn ok_data(data: serde_json::Value) -> BwResponse {
    BwResponse {
        success: true,
        data,
        message: None,
        error_code: None,
    }
}

fn err(msg: &str) -> BwResponse {
    BwResponse {
        success: false,
        data: serde_json::Value::Null,
        message: Some(msg.into()),
        error_code: None,
    }
}

fn build_store(responses: Vec<BwResponse>) -> (Arc<BitwardenStore>, Arc<ScriptedExec>) {
    // Responses are popped from the END, so reverse for natural order.
    let mut responses = responses;
    responses.reverse();
    let exec = Arc::new(ScriptedExec::new(responses));
    let kc = Keychain::in_memory("test");
    let store = Arc::new(BitwardenStore::new(exec.clone(), kc));
    (store, exec)
}

fn cipher_json(id: &str, name: &str, password: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "type": 1,
        "revisionDate": "2025-04-01T12:00:00.000Z",
        "login": { "password": password }
    })
}

#[test]
fn map_bw_message_classifies_known_strings() {
    assert!(matches!(
        map_bw_message("Vault is locked."),
        SecretError::Locked
    ));
    assert!(matches!(
        map_bw_message("You are not logged in."),
        SecretError::LoggedOut
    ));
    assert!(matches!(
        map_bw_message("Not found."),
        SecretError::NotFound
    ));
    assert!(matches!(
        map_bw_message("More than one result was found."),
        SecretError::Ambiguous { .. }
    ));
    assert!(matches!(
        map_bw_message("something exploded"),
        SecretError::Backend { .. }
    ));
}

#[tokio::test]
async fn unavailable_store_short_circuits() {
    let store = BitwardenStore::new_unavailable("bw not installed");
    let s = store.status().await;
    assert!(matches!(s, StoreStatus::Unavailable { .. }));
    assert!(matches!(
        store.get("k").await,
        Err(SecretError::Locked) | Err(SecretError::Backend { .. })
    ));
}

#[tokio::test]
async fn unlock_password_path_stores_session() {
    // Order: status (unauthenticated) → config server → status → login → unlock.
    let (store, exec) = build_store(vec![
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // config server
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message", "title": "logged in" })),
        ok_data(json!("session-abc123")),
    ]);
    store
        .unlock(UnlockCredentials {
            master_password: SecretString::from("pw"),
            two_factor_token: None,
            email: Some("u@x.com".into()),
            api_key_client_id: None,
            api_key_client_secret: None,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .session
            .read()
            .await
            .as_ref()
            .map(|s| s.expose_secret().to_string()),
        Some("session-abc123".into())
    );

    let calls = exec.call_args();
    assert_eq!(calls[0], vec!["status"]);
    assert_eq!(calls[1][0], "config");
    // Login lives at index 3.
    assert_eq!(calls[3][0], "login");
    assert!(calls[3].contains(&"--passwordenv".to_string()));
    // Master password must NEVER appear in argv.
    assert!(calls.iter().all(|c| !c.iter().any(|a| a == "pw")));
}

#[tokio::test]
async fn unlock_when_already_logged_in_skips_login_step() {
    // status (locked) → config server → status (locked) → unlock.
    let (store, exec) = build_store(vec![
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!({ "object": "message" })),
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!("session-xyz")),
    ]);
    store
        .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
        .await
        .unwrap();
    let calls = exec.call_args();
    assert_eq!(calls.len(), 4, "got {:?}", calls);
    // No login call — only status, config, status, unlock.
    assert!(!calls.iter().any(|c| c[0] == "login"));
    assert_eq!(calls[3][0], "unlock");
}

#[tokio::test]
async fn unlock_api_key_flow_persists_creds() {
    let (store, _exec) = build_store(vec![
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // config server
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // login
        ok_data(json!("session-api-1")),         // unlock
    ]);
    store
        .unlock(UnlockCredentials {
            master_password: SecretString::from("pw"),
            two_factor_token: None,
            email: None,
            api_key_client_id: Some(SecretString::from("client-id")),
            api_key_client_secret: Some(SecretString::from("client-secret")),
        })
        .await
        .unwrap();
    // Keychain is in-memory; ensure both api creds got persisted.
    assert_eq!(
        store.keychain.get(BW_KEYRING_API_CLIENT_ID).unwrap(),
        "client-id"
    );
    assert_eq!(
        store.keychain.get(BW_KEYRING_API_CLIENT_SECRET).unwrap(),
        "client-secret"
    );
}

#[tokio::test]
async fn save_creates_when_no_existing_match() {
    let (store, exec) = build_store(vec![
        ok_data(json!([])), // find_by_name → empty
        // No folder requested → no list folders call.
        ok_data(cipher_json("new-id", "openai", "sk-foo")), // create
        ok_data(serde_json::Value::Null),                   // sync
    ]);
    // Pre-set session so we don't need to unlock.
    *store.session.write().await = Some(SecretString::from("test-session"));

    let r = store
        .save(SaveRequest {
            name: "openai".into(),
            value: SecretString::from("sk-foo"),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: false,
        })
        .await
        .unwrap();
    assert_eq!(r.item_id, "new-id");
    assert_eq!(r.name, "openai");

    let calls = exec.call_args();
    assert_eq!(calls[1][0], "create");
    assert_eq!(calls[1][1], "item");
    // Value must travel as base64-encoded JSON in argv (Bitwarden's
    // documented input shape) — but the raw plaintext must NOT appear.
    assert!(calls
        .iter()
        .all(|c| !c.iter().any(|a| a.contains("sk-foo"))));
}

#[tokio::test]
async fn save_without_overwrite_errors_when_exists() {
    let (store, _exec) = build_store(vec![ok_data(json!([cipher_json(
        "existing-id",
        "openai",
        "old"
    ),]))]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    let err = store
        .save(SaveRequest {
            name: "openai".into(),
            value: SecretString::from("new"),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, SecretError::AlreadyExists));
}

#[tokio::test]
async fn save_with_overwrite_calls_edit() {
    let (store, exec) = build_store(vec![
        ok_data(json!([cipher_json("existing-id", "openai", "old")])),
        ok_data(cipher_json("existing-id", "openai", "new")),
        ok_data(serde_json::Value::Null), // sync
    ]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    store
        .save(SaveRequest {
            name: "openai".into(),
            value: SecretString::from("new"),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: true,
        })
        .await
        .unwrap();

    let calls = exec.call_args();
    assert_eq!(calls[1][0], "edit");
    assert_eq!(calls[1][1], "item");
    assert_eq!(calls[1][2], "existing-id");
}

#[tokio::test]
async fn get_returns_password_from_login_field() {
    let (store, _) = build_store(vec![ok_data(cipher_json("abc", "openai", "sk-prod"))]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    let v = store.get("openai").await.unwrap();
    assert_eq!(v.value.expose_secret(), "sk-prod");
    assert_eq!(v.item_id, "abc");
    assert_eq!(v.name, "openai");
}

#[tokio::test]
async fn get_locked_when_no_session() {
    let (store, _) = build_store(vec![]);
    // No session set.
    let err = store.get("openai").await.unwrap_err();
    assert!(matches!(err, SecretError::Locked));
}

#[tokio::test]
async fn list_filters_to_logins_and_sorts() {
    let (store, _) = build_store(vec![ok_data(json!([
        { "id": "a", "name": "zeta",  "type": 1, "login": { "password": "x" } },
        { "id": "b", "name": "alpha", "type": 1, "login": { "password": "x" } },
        { "id": "c", "name": "card",  "type": 3 }, // card — filtered
    ]))]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    let items = store.list(ListFilter::default()).await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].name, "alpha");
    assert_eq!(items[1].name, "zeta");
}

#[tokio::test]
async fn search_passes_query_to_bw() {
    let (store, exec) = build_store(vec![ok_data(json!([
        { "id": "a", "name": "openai-prod", "type": 1, "login": { "password": "x" } }
    ]))]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    let items = store.search("openai").await.unwrap();
    assert_eq!(items.len(), 1);
    let calls = exec.call_args();
    assert_eq!(calls[0], vec!["list", "items", "--search", "openai"]);
}

#[tokio::test]
async fn delete_finds_then_deletes() {
    let (store, exec) = build_store(vec![
        ok_data(cipher_json("doomed", "old", "v")),
        ok_data(json!({ "ok": true })),
        ok_data(serde_json::Value::Null), // sync
    ]);
    *store.session.write().await = Some(SecretString::from("test-session"));

    store.delete("old").await.unwrap();
    let calls = exec.call_args();
    assert_eq!(calls[1][0], "delete");
    assert_eq!(calls[1][1], "item");
    assert_eq!(calls[1][2], "doomed");
    assert!(calls[1].contains(&"--permanent".to_string()));
}

#[tokio::test]
async fn vault_locked_propagates_through_data_call() {
    let (store, _) = build_store(vec![err("Vault is locked.")]);
    *store.session.write().await = Some(SecretString::from("test-session"));
    let err = store.get("anything").await.unwrap_err();
    assert!(matches!(err, SecretError::Locked));
}

#[test]
fn unwrap_data_envelope_handles_known_shapes() {
    // {object: "list", data: [...]}  → returns the array
    let listed = json!({ "object": "list", "data": [{"id": "1"}] });
    let inner = unwrap_data_envelope(listed);
    assert!(inner.is_array());
    assert_eq!(inner[0]["id"], "1");

    // {object: "template", template: {status: "..."}}  → returns the object
    let wrapped_status = json!({ "object": "template", "template": {"status": "locked"} });
    let inner = unwrap_data_envelope(wrapped_status);
    assert_eq!(inner["status"], "locked");

    // Flat (legacy) → returned as-is
    let flat = json!({ "status": "locked" });
    let inner = unwrap_data_envelope(flat);
    assert_eq!(inner["status"], "locked");
}

#[test]
fn is_valid_http_url_accepts_known_shapes() {
    assert!(is_valid_http_url("https://vault.bitwarden.com"));
    assert!(is_valid_http_url("http://vaultwarden.local:8080"));
    assert!(is_valid_http_url("https://vault.example.org/path"));
}

#[test]
fn is_valid_http_url_rejects_garbage() {
    assert!(!is_valid_http_url(""));
    assert!(!is_valid_http_url("ftp://vault.example.com"));
    assert!(!is_valid_http_url("javascript:alert(1)"));
    assert!(!is_valid_http_url("https://"));
    assert!(!is_valid_http_url("vault.example.com"));
}

#[tokio::test]
async fn set_server_url_returns_true_on_change_only() {
    let store = BitwardenStore::new(Arc::new(NoopBwExec), Keychain::in_memory("t"));
    assert!(store.set_server_url("https://example.com".into()));
    assert!(!store.set_server_url("https://example.com".into()));
    assert!(store.set_server_url("https://other.com".into()));
    assert_eq!(store.server_url(), "https://other.com");
}

#[tokio::test]
async fn unlock_runs_config_server_before_login() {
    let (store, exec) = build_store(vec![
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // config server
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // login
        ok_data(json!("session-server")),
    ]);
    store.set_server_url("https://vault.example.com".into());
    store
        .unlock(UnlockCredentials {
            master_password: SecretString::from("pw"),
            two_factor_token: None,
            email: Some("u@x.com".into()),
            api_key_client_id: None,
            api_key_client_secret: None,
        })
        .await
        .unwrap();
    let calls = exec.call_args();
    assert_eq!(calls[1][0], "config");
    assert_eq!(calls[1][1], "server");
    assert_eq!(calls[1][2], "https://vault.example.com");
}

#[tokio::test]
async fn unlock_extracts_token_from_modern_wrapper() {
    // status (locked) → config server → status (locked) → unlock returns wrapper
    let (store, _) = build_store(vec![
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!({ "object": "message" })), // config server
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!({ "object": "unlock", "raw": "session-from-wrapper" })),
    ]);
    store
        .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
        .await
        .unwrap();
    assert_eq!(
        store
            .session
            .read()
            .await
            .as_ref()
            .map(|s| s.expose_secret().to_string()),
        Some("session-from-wrapper".into())
    );
}

#[tokio::test]
async fn unlock_extracts_token_from_template_wrapper() {
    let (store, _) = build_store(vec![
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!({ "object": "message" })),
        ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
        ok_data(json!({ "template": { "raw": "session-from-template" } })),
    ]);
    store
        .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
        .await
        .unwrap();
    assert_eq!(
        store
            .session
            .read()
            .await
            .as_ref()
            .map(|s| s.expose_secret().to_string()),
        Some("session-from-template".into())
    );
}

#[tokio::test]
async fn login_already_logged_in_is_treated_as_success() {
    // Status reports unauthenticated → triggers login → bw returns
    // "already logged in" → we should soft-succeed and proceed to unlock.
    let (store, _) = build_store(vec![
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })),
        ok_data(json!({ "status": "unauthenticated" })),
        err("You are already logged in as u@x.com."),
        ok_data(json!("session-after-already")),
    ]);
    store
        .unlock(UnlockCredentials {
            master_password: SecretString::from("pw"),
            two_factor_token: None,
            email: Some("u@x.com".into()),
            api_key_client_id: None,
            api_key_client_secret: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn unlock_forces_logout_when_server_changed() {
    // status (logged in to old) → logout → config server (new) → status (logged out) → login → unlock
    let (store, exec) = build_store(vec![
        ok_data(json!({ "status": "locked", "serverUrl": "https://old.example" })),
        ok_data(json!({ "object": "message" })), // logout
        ok_data(json!({ "object": "message" })), // config server
        ok_data(json!({ "status": "unauthenticated" })),
        ok_data(json!({ "object": "message" })), // login
        ok_data(json!("session-after-switch")),
    ]);
    store.set_server_url("https://new.example".into());
    store
        .unlock(UnlockCredentials {
            master_password: SecretString::from("pw"),
            two_factor_token: None,
            email: Some("u@x.com".into()),
            api_key_client_id: None,
            api_key_client_secret: None,
        })
        .await
        .unwrap();
    let calls = exec.call_args();
    // logout call appears before config server.
    let pos_logout = calls.iter().position(|c| c[0] == "logout").unwrap();
    let pos_config = calls.iter().position(|c| c[0] == "config").unwrap();
    assert!(pos_logout < pos_config, "logout must precede config server");
}

#[tokio::test]
async fn unlock_rejects_invalid_server_url() {
    let store = BitwardenStore::new(Arc::new(NoopBwExec), Keychain::in_memory("t"));
    store.set_server_url("ftp://bad".into());
    let err = store
        .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
        .await
        .unwrap_err();
    assert!(matches!(err, SecretError::BadRequest { .. }));
}

#[tokio::test]
async fn bw_status_unwraps_template_envelope() {
    // Modern bw wraps the real payload in {object, template, ...}.
    let (store, _) = build_store(vec![ok_data(json!({
        "object": "template",
        "template": {
            "status": "unauthenticated",
            "serverUrl": "https://vault.example.com"
        }
    }))]);
    let s = store.bw_status().await.unwrap();
    assert_eq!(s.status, "unauthenticated");
    assert_eq!(s.server_url.as_deref(), Some("https://vault.example.com"));
}

#[tokio::test]
async fn bw_status_falls_back_to_legacy_flat_shape() {
    // Older bw returned status fields at the top level.
    let (store, _) = build_store(vec![ok_data(json!({
        "status": "locked",
        "userEmail": "u@x.com"
    }))]);
    let s = store.bw_status().await.unwrap();
    assert_eq!(s.status, "locked");
    assert_eq!(s.user_email.as_deref(), Some("u@x.com"));
}

#[tokio::test]
async fn status_includes_server_endpoint() {
    let (store, _exec) = build_store(vec![ok_data(
        json!({ "status": "locked", "userEmail": "u@x.com",
                        "serverUrl": "https://self.host" }),
    )]);
    store.set_server_url("https://self.host".into());
    match store.status().await {
        StoreStatus::Locked { endpoint } => {
            assert_eq!(endpoint.as_deref(), Some("https://self.host"));
        }
        other => panic!("expected Locked with endpoint, got {:?}", other),
    }
}

#[test]
fn first_executable_returns_none_for_no_candidates() {
    assert!(first_executable(Vec::<std::path::PathBuf>::new()).is_none());
}

#[cfg(unix)]
#[test]
fn first_executable_skips_missing_and_non_exec_then_finds_exec() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope");
    let plain = tmp.path().join("plain");
    fs::write(&plain, b"#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
    let exe = tmp.path().join("bw");
    fs::write(&exe, b"#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();

    let found = first_executable(vec![missing, plain, exe.clone()]);
    assert_eq!(found.as_ref(), Some(&exe));
}

#[cfg(unix)]
#[test]
fn is_executable_file_rejects_directory() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_executable_file(tmp.path()));
}

#[cfg(target_os = "macos")]
#[test]
fn fallback_paths_include_known_macos_locations() {
    let paths = fallback_bw_paths();
    let strs: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(strs.contains(&"/opt/homebrew/bin/bw".to_string()));
    assert!(strs.contains(&"/usr/local/bin/bw".to_string()));
}

#[tokio::test]
async fn logout_clears_session_and_keyring() {
    let (store, _) = build_store(vec![ok_data(json!({ "ok": true }))]);
    *store.session.write().await = Some(SecretString::from("token"));
    store.keychain.set(BW_KEYRING_SESSION, "token").unwrap();
    store.keychain.set(BW_KEYRING_API_CLIENT_ID, "id").unwrap();

    store.logout().await.unwrap();
    assert!(store.session.read().await.is_none());
    assert!(matches!(
        store.keychain.get(BW_KEYRING_SESSION),
        Err(KeychainError::NotFound)
    ));
    assert!(matches!(
        store.keychain.get(BW_KEYRING_API_CLIENT_ID),
        Err(KeychainError::NotFound)
    ));
}
