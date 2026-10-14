//! REST handlers for `/api/secrets/*`.
//!
//! Adapter pattern: each handler deserializes the body, calls
//! `SecretStore` on the active backend, and serializes the result. No
//! business logic. The reveal endpoint (`POST .../value`) is the only path
//! that emits plaintext; all others return metadata-only payloads.
//!
//! Logging discipline:
//! - The `value` field never gets `.into()`'d into a `tracing` field.
//! - On error we log only the variant kind, never the message of a
//!   `BadRequest` (which could echo back attacker-controlled input).

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::api::routes::{check_auth, AppState};
use crate::secrets::{
    ListFilter, SaveRequest, SecretError, SecretRef, SecretString, UnlockCredentials,
};

// ── Error mapping ───────────────────────────────────────────────────

/// Strip C0 control characters (0x00..0x1F) and DEL (0x7F) so an attacker
/// who controls config/CLI input can't inject ANSI/OSC escapes that would
/// rewrite the user's terminal title or move the cursor when the CLI
/// prints the error message verbatim.
fn sanitize_echoed(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let v = *c as u32;
            !(v < 0x20 || v == 0x7f)
        })
        .collect()
}

fn map_error(e: SecretError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match &e {
        SecretError::LoggedOut => StatusCode::UNAUTHORIZED,
        SecretError::Locked => StatusCode::LOCKED,
        SecretError::NotFound => StatusCode::NOT_FOUND,
        SecretError::Ambiguous { .. } => StatusCode::CONFLICT,
        SecretError::AlreadyExists => StatusCode::CONFLICT,
        SecretError::BadRequest { .. } => StatusCode::BAD_REQUEST,
        SecretError::Backend { .. } => StatusCode::SERVICE_UNAVAILABLE,
    };
    // `message` may interpolate user-controlled strings (server URL, bw stderr); scrub control chars before the CLI prints them.
    let message = match &e {
        SecretError::LoggedOut => "logged out — sign in again".to_string(),
        SecretError::Locked => "vault is locked".to_string(),
        SecretError::NotFound => "not found".to_string(),
        SecretError::Ambiguous { candidates } => {
            format!("ambiguous match ({} candidates)", candidates.len())
        }
        SecretError::AlreadyExists => "already exists".to_string(),
        SecretError::BadRequest { message } => sanitize_echoed(message),
        SecretError::Backend { message } => sanitize_echoed(message),
    };
    let body = serde_json::json!({
        "error": {
            "kind": e,
            "message": message,
        }
    });
    (status, Json(body))
}

// ── Status / lifecycle ──────────────────────────────────────────────

pub async fn status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let store = state.svc.secrets.read().await;
    let st = store.status().await;
    let auto_unlock_capable = store.capabilities().auto_unlock;
    Ok(Json(serde_json::json!({
        "backend": store.id(),
        "status": st,
        "auto_unlock_capable": auto_unlock_capable,
        "master_password_saved": crate::secrets::master_password_saved(&**store),
    })))
}

#[derive(Deserialize)]
pub struct UnlockBody {
    /// Master password / vault key.
    pub master_password: String,
    #[serde(default)]
    pub two_factor_token: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub api_key_client_id: Option<String>,
    #[serde(default)]
    pub api_key_client_secret: Option<String>,
    /// Explicit "remember this for headless unlock" intent. Absent ⇒ false, so
    /// an ordinary sign-in can never arm auto-unlock by omission.
    #[serde(default)]
    pub persist_for_auto_unlock: bool,
}

impl UnlockBody {
    pub fn into_credentials(self) -> UnlockCredentials {
        UnlockCredentials {
            master_password: SecretString::from(self.master_password),
            two_factor_token: self.two_factor_token.map(SecretString::from),
            email: self.email,
            api_key_client_id: self.api_key_client_id.map(SecretString::from),
            api_key_client_secret: self.api_key_client_secret.map(SecretString::from),
            persist_for_auto_unlock: self.persist_for_auto_unlock,
        }
    }
}

pub async fn unlock(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<UnlockBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let creds = body.into_credentials();
    let store = state.svc.secrets.read().await;
    crate::secrets::check_persist_consent(&**store, creds.persist_for_auto_unlock)
        .map_err(map_error)?;
    store.unlock(creds).await.map_err(map_error)?;
    let st = store.status().await;
    Ok(Json(serde_json::json!({"ok": true, "status": st})))
}

pub async fn lock(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    store.lock().await.map_err(map_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn logout(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    store.logout().await.map_err(map_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Re-run the active backend's `init` — for Bitwarden this attempts an
/// auto-unlock using the master password persisted in the local `.secrets`
/// store on the previous successful login. No-op for backends without
/// persisted creds. Used by the CLI `mem secret unlock` (no-password form)
/// and by the UI when it wants to re-sync session state without prompting.
pub async fn auto_unlock(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    if let Err(e) = store.init().await {
        return Err(map_error(e));
    }
    let st = store.status().await;
    let unlocked = matches!(st, crate::secrets::StoreStatus::Unlocked { .. });
    Ok(Json(serde_json::json!({
        "ok": unlocked,
        "backend": store.id(),
        "status": st,
    })))
}

/// Pull latest from the remote vault. No-op for backends with no remote
/// (Local file store, Mock). Bitwarden runs `bw sync`.
pub async fn sync(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    store.sync().await.map_err(map_error)?;
    Ok(Json(serde_json::json!({"ok": true, "backend": store.id()})))
}

// ── CRUD ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct ItemsResponse {
    pub items: Vec<SecretRef>,
}

pub async fn list_items(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<ItemsResponse>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    let items = if let Some(query) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        store.search(query).await.map_err(map_error)?
    } else {
        store
            .list(ListFilter {
                folder: q.folder,
                organization: q.organization,
                collection: q.collection,
                limit: q.limit,
            })
            .await
            .map_err(map_error)?
    };
    Ok(Json(ItemsResponse { items }))
}

pub async fn get_item_metadata(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<SecretRef>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    let v = store.get(&name).await.map_err(map_error)?;
    Ok(Json(SecretRef {
        item_id: v.item_id,
        name: v.name,
        folder: None,
        organization: None,
        collection: None,
        last_modified: None,
    }))
}

#[derive(Serialize)]
pub struct ValueResponse {
    pub item_id: String,
    pub name: String,
    pub value: String,
    pub notes: Option<String>,
}

/// Reveal the secret value. POST (not GET) so the name stays out of URL
/// access logs and so we can require a body in the future without breaking
/// clients.
pub async fn reveal_value(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<ValueResponse>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    let v = store.get(&name).await.map_err(map_error)?;
    Ok(Json(ValueResponse {
        item_id: v.item_id,
        name: v.name,
        value: v.value.expose_secret().to_string(),
        notes: v.notes,
    }))
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

pub async fn save_item(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveBody>,
) -> Result<Json<SecretRef>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;

    // "personal vault" — leave it as None so the backend doesn't try to attach to a non-existent org.
    let cfg = crate::config::load_config();
    let default_org = some_if_nonempty(&cfg.integrations.secrets.default_organization);
    let default_coll = some_if_nonempty(&cfg.integrations.secrets.default_collection);
    let organization = body.organization.or(default_org);
    let collection = body.collection.or(default_coll);

    let req = SaveRequest {
        name: body.name,
        value: SecretString::from(body.value),
        notes: body.notes,
        folder: body.folder,
        organization,
        collection,
        overwrite: body.overwrite,
    };
    let r = store.save(req).await.map_err(map_error)?;
    Ok(Json(r))
}

fn some_if_nonempty(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub async fn delete_item(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    store.delete(&name).await.map_err(map_error)?;
    Ok(Json(serde_json::json!({"ok": true, "name": name})))
}

pub async fn organizations(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    let orgs = store.organizations().await.map_err(map_error)?;
    Ok(Json(serde_json::json!({"organizations": orgs})))
}

#[derive(Deserialize)]
pub struct CollectionsQuery {
    #[serde(default)]
    pub organization: Option<String>,
}

pub async fn collections(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<CollectionsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let store = state.svc.secrets.read().await;
    let colls = store
        .collections(q.organization.as_deref())
        .await
        .map_err(map_error)?;
    Ok(Json(serde_json::json!({"collections": colls})))
}

#[derive(Deserialize)]
pub struct SwitchBackendBody {
    pub backend: String,
    /// Optional Bitwarden / Vaultwarden server URL — only honored when
    /// `backend == "bitwarden"`.
    #[serde(default)]
    pub bitwarden_server_url: Option<String>,
}

/// Atomic backend swap. Persists the choice to config IMMEDIATELY (no
/// "Save" step) and reconstructs the active store. The previous backend's
/// in-process state is dropped, but persisted creds (e.g. Bitwarden session
/// in `.secrets`) are kept so switching back is seamless.
pub async fn switch_backend(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SwitchBackendBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }

    // registered into the secret registry (e.g. the Bitwarden plugin).
    if crate::secrets::get_factory(&body.backend).is_none()
        && !state.svc.secret_registry.contains(&body.backend)
    {
        return Err(map_error(SecretError::BadRequest {
            message: format!("unknown backend: {}", body.backend),
        }));
    }

    if let Some(url) = body.bitwarden_server_url.as_deref() {
        if !crate::secrets::is_valid_http_url(url) {
            return Err(map_error(SecretError::BadRequest {
                message: format!("invalid server URL: {}", url),
            }));
        }
    }

    // Persist to config first — if the daemon dies right after the swap we still want the new choice to stick.
    {
        let mut cfg = crate::config::load_config();
        cfg.integrations.secrets.backend = body.backend.clone();
        // config.plugins.bitwarden.serverUrl); native Bitwarden is gone.
        if let Some(url) = body.bitwarden_server_url.as_ref() {
            let entry = cfg
                .plugins
                .entry("bitwarden".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if let Some(obj) = entry.as_object_mut() {
                obj.insert("serverUrl".into(), serde_json::json!(url));
            }
        }
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            return Err(map_error(SecretError::Backend {
                message: format!("save config: {}", e),
            }));
        }
    }

    // Build the new backend, run init, then atomically swap. Init failures are surfaced — the caller asked to switch and deserves to know whether the new backend is actually usable.
    let cfg = crate::config::load_config();
    let new_store =
        crate::secrets::build_active_store(&cfg.integrations.secrets, &state.svc.secret_registry);
    if let Err(e) = new_store.init().await {
        log::warn!("[secrets] init on swap to '{}' failed: {}", body.backend, e);
    }
    {
        let mut guard = state.svc.secrets.write().await;
        *guard = new_store;
    }

    // Read the new status so the caller can update its UI in one round-trip.
    let store = state.svc.secrets.read().await;
    let st = store.status().await;
    Ok(Json(serde_json::json!({
        "ok": true,
        "backend": store.id(),
        "status": st,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::StoreStatus;

    use crate::secrets::{
        ListFilter, SaveRequest, SecretRef, SecretStore, SecretValue, StoreCapabilities,
        UnlockCredentials,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Records the consent bit exactly as the backend receives it.
    struct ConsentRecordingStore {
        caps: StoreCapabilities,
        saw_consent: AtomicBool,
    }

    #[async_trait]
    impl SecretStore for ConsentRecordingStore {
        fn id(&self) -> &'static str {
            "consent-recording"
        }
        fn capabilities(&self) -> StoreCapabilities {
            self.caps
        }
        fn auto_unlock_armed(&self) -> bool {
            false
        }
        async fn status(&self) -> StoreStatus {
            StoreStatus::Locked { endpoint: None }
        }
        async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
            self.saw_consent
                .store(creds.persist_for_auto_unlock, Ordering::SeqCst);
            Ok(())
        }
        async fn lock(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn logout(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
            Err(SecretError::Locked)
        }
        async fn get(&self, _: &str) -> Result<SecretValue, SecretError> {
            Err(SecretError::Locked)
        }
        async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn delete(&self, _: &str) -> Result<(), SecretError> {
            Err(SecretError::Locked)
        }
    }

    fn recording(caps: StoreCapabilities) -> ConsentRecordingStore {
        ConsentRecordingStore {
            caps,
            saw_consent: AtomicBool::new(false),
        }
    }

    /// Drives the real wire shape the CLI and UI send, through the real
    /// conversion, into a backend that records what arrived.
    async fn consent_seen_by_backend(wire: &str, caps: StoreCapabilities) -> bool {
        let body: UnlockBody = serde_json::from_str(wire).expect("parse unlock body");
        let creds = body.into_credentials();
        let store = recording(caps);
        crate::secrets::check_persist_consent(&store, creds.persist_for_auto_unlock)
            .expect("consent allowed");
        store.unlock(creds).await.expect("unlock");
        store.saw_consent.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn explicit_arm_request_reaches_the_backend_as_consent() {
        let wire = r#"{"master_password":"synthetic-not-a-credential-0000","persist_for_auto_unlock":true}"#;
        assert!(consent_seen_by_backend(wire, StoreCapabilities::AUTO_UNLOCK).await);
    }

    #[tokio::test]
    async fn ordinary_unlock_reaches_the_backend_without_consent() {
        // Password login, API-key login, and a bare arm-less body all omit the
        // field; omission must arrive as false, never as "unspecified".
        let wires = [
            r#"{"master_password":"synthetic-not-a-credential-0000","email":"user@example.test"}"#,
            r#"{"master_password":"synthetic-not-a-credential-0000","api_key_client_id":"cid","api_key_client_secret":"csec"}"#,
            r#"{"master_password":"synthetic-not-a-credential-0000","persist_for_auto_unlock":false}"#,
        ];
        for wire in wires {
            assert!(
                !consent_seen_by_backend(wire, StoreCapabilities::AUTO_UNLOCK).await,
                "wire must not arm: {wire}"
            );
        }
    }

    #[tokio::test]
    async fn a_non_declaring_backend_refuses_the_consent_bit() {
        let wire = r#"{"master_password":"synthetic-not-a-credential-0000","persist_for_auto_unlock":true}"#;
        let body: UnlockBody = serde_json::from_str(wire).expect("parse unlock body");
        let creds = body.into_credentials();
        let store = recording(StoreCapabilities::NONE);
        let refused = crate::secrets::check_persist_consent(&store, creds.persist_for_auto_unlock);
        assert!(matches!(refused, Err(SecretError::BadRequest { .. })));
        assert_eq!(map_error(refused.unwrap_err()).0, StatusCode::BAD_REQUEST);
        assert!(!store.saw_consent.load(Ordering::SeqCst));
    }

    /// The full chain a real caller drives: wire JSON, into credentials, into the
    /// argument the JS backend receives.
    fn consent_across_the_js_boundary(wire: &str) -> serde_json::Value {
        let body: UnlockBody = serde_json::from_str(wire).expect("parse unlock body");
        crate::secrets::js_backend::unlock_args(&body.into_credentials())
    }

    #[test]
    fn ordinary_unlock_serializes_an_explicit_false_to_the_plugin() {
        // Omission must not reach the plugin as an absent field to guess at.
        let wires = [
            r#"{"master_password":"synthetic-not-a-credential-0000","email":"user@example.test"}"#,
            r#"{"master_password":"synthetic-not-a-credential-0000","api_key_client_id":"cid","api_key_client_secret":"csec"}"#,
        ];
        for wire in wires {
            let args = consent_across_the_js_boundary(wire);
            let field = args
                .get("persistForAutoUnlock")
                .unwrap_or_else(|| panic!("field must be present for {wire}"));
            assert!(field.is_boolean(), "must be a bool, got {field} for {wire}");
            assert_eq!(*field, serde_json::json!(false), "wire: {wire}");
            assert!(args.to_string().contains("\"persistForAutoUnlock\":false"));
        }
    }

    #[test]
    fn explicit_arm_serializes_true_to_the_plugin() {
        let wire = r#"{"master_password":"synthetic-not-a-credential-0000","persist_for_auto_unlock":true}"#;
        let args = consent_across_the_js_boundary(wire);
        assert_eq!(args["persistForAutoUnlock"], serde_json::json!(true));
        assert!(args.to_string().contains("\"persistForAutoUnlock\":true"));
    }

    /// Smoke test for the error→HTTP mapping. Each variant must land on a
    /// stable status code so CLI exit codes stay consistent across releases.
    #[test]
    fn error_status_mapping() {
        let cases = [
            (SecretError::LoggedOut, StatusCode::UNAUTHORIZED),
            (SecretError::Locked, StatusCode::LOCKED),
            (SecretError::NotFound, StatusCode::NOT_FOUND),
            (
                SecretError::Ambiguous {
                    candidates: vec!["a".into()],
                },
                StatusCode::CONFLICT,
            ),
            (SecretError::AlreadyExists, StatusCode::CONFLICT),
            (
                SecretError::BadRequest {
                    message: "bad".into(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                SecretError::Backend {
                    message: "down".into(),
                },
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ];
        for (e, expected) in cases {
            let (got, _) = map_error(e);
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn sanitize_echoed_strips_control_chars() {
        // ANSI title-set: "\x1b]0;NEW TITLE\x07"
        let evil = "url \x1b]0;PWNED\x07 here";
        let cleaned = sanitize_echoed(evil);
        assert!(!cleaned.contains('\x1b'));
        assert!(!cleaned.contains('\x07'));
        assert!(cleaned.contains("url "));
        assert!(cleaned.contains(" here"));
    }

    #[test]
    fn sanitize_echoed_keeps_normal_text() {
        let s = "normal message — with unicode 🔐";
        assert_eq!(sanitize_echoed(s), s);
    }

    #[test]
    fn sanitize_echoed_drops_del_byte() {
        assert_eq!(sanitize_echoed("a\x7fb"), "ab");
    }

    #[test]
    fn store_status_serialization_uses_snake_case_tag() {
        let s = StoreStatus::Unlocked {
            user: Some("a@b.com".into()),
            transient: false,
            endpoint: Some("https://vault.bitwarden.com".into()),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["status"], "unlocked");
        assert_eq!(v["user"], "a@b.com");
        assert_eq!(v["endpoint"], "https://vault.bitwarden.com");
    }
}
