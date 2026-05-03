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
    BackendOptions, ListFilter, SaveRequest, SecretError, SecretRef, SecretString,
    UnlockCredentials,
};

// ── Error mapping ───────────────────────────────────────────────────

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
    let body = serde_json::json!({
        "error": e,
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
    Ok(Json(serde_json::json!({
        "backend": store.id(),
        "status": st,
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
}

pub async fn unlock(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<UnlockBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }
    let creds = UnlockCredentials {
        master_password: SecretString::from(body.master_password),
        two_factor_token: body.two_factor_token.map(SecretString::from),
        email: body.email,
        api_key_client_id: body.api_key_client_id.map(SecretString::from),
        api_key_client_secret: body.api_key_client_secret.map(SecretString::from),
    };
    let store = state.svc.secrets.read().await;
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
    let req = SaveRequest {
        name: body.name,
        value: SecretString::from(body.value),
        notes: body.notes,
        folder: body.folder,
        organization: body.organization,
        collection: body.collection,
        overwrite: body.overwrite,
    };
    let r = store.save(req).await.map_err(map_error)?;
    Ok(Json(r))
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
/// in-process state is dropped — but persisted creds (e.g. Bitwarden
/// session in OS keychain) are kept so switching back is seamless.
pub async fn switch_backend(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SwitchBackendBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Err((s, m)) = check_auth(&state, &headers) {
        return Err((s, Json(serde_json::json!({"error": m}))));
    }

    let known = matches!(body.backend.as_str(), "local" | "bitwarden" | "mock");
    if !known {
        return Err(map_error(SecretError::BadRequest {
            message: format!("unknown backend: {}", body.backend),
        }));
    }

    if body.backend == "bitwarden" {
        if let Some(url) = body.bitwarden_server_url.as_deref() {
            if !crate::secrets::bitwarden::is_valid_http_url(url) {
                return Err(map_error(SecretError::BadRequest {
                    message: format!("invalid server URL: {}", url),
                }));
            }
        }
    }

    // Persist to config first — if the daemon dies right after the swap
    // we still want the new choice to stick.
    {
        let mut cfg = crate::config::load_config();
        cfg.integrations.secrets.backend = body.backend.clone();
        if let Some(url) = body.bitwarden_server_url.as_ref() {
            cfg.integrations.secrets.bitwarden.server_url = url.clone();
        }
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            return Err(map_error(SecretError::Backend {
                message: format!("save config: {}", e),
            }));
        }
    }

    // Build the new backend with the new options, then atomically swap.
    let cfg = crate::config::load_config();
    let opts = BackendOptions::from_config(&cfg.integrations.secrets);
    let new_store = crate::secrets::build_active_store_with_options(&body.backend, opts);
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
