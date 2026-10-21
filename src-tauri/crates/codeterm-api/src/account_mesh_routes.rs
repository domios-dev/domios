use super::account_routes::{admit, unavailable};
use super::{auth::CallerIdentity, routes::AppState};
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use codeterm_account::account_mesh::wire::MeshTarget;
use codeterm_service_core::service::account_mesh::{
    AccountMeshOutcome, AccountMeshSnapshot, RevokedResultView, UpdatedResultView,
};
use serde::Deserialize;
type Error = (StatusCode, String);

/// Local-only Account Mesh facade.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/account/mesh", get(state))
        .route("/api/account/mesh/enroll", post(enroll))
        .route("/api/account/mesh/list", post(list))
        .route("/api/account/mesh/rename", post(rename_self))
        .route("/api/account/mesh/revoke", post(revoke))
        .route(
            "/api/account/mesh/limit-notice/ack",
            post(acknowledge_limit),
        )
}

/// `account_mesh_state`
pub async fn state(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<AccountMeshSnapshot>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account_mesh_snapshot()
            .await
            .map_err(unavailable)?,
    ))
}

/// Explicit enrollment retry; a `mesh.machines` refusal carries its request id.
pub async fn enroll(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<AccountMeshOutcome<()>>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state.svc.account_mesh_enroll().await.map_err(unavailable)?,
    ))
}

/// Online registry read; the result lands in the snapshot as a stale observation.
pub async fn list(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<AccountMeshOutcome<()>>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state.svc.account_mesh_list().await.map_err(unavailable)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rename {
    expected_metadata_revision: u64,
    display_name: String,
}

/// `account_mesh_rename_self`
pub async fn rename_self(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Rename>,
) -> Result<Json<AccountMeshOutcome<UpdatedResultView>>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account_mesh_rename_self(input.expected_metadata_revision, input.display_name)
            .await
            .map_err(unavailable)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revoke {
    target: MeshTarget,
}

/// `account_mesh_revoke`: releases the server slot; local unpair stays `mesh_disconnect`.
pub async fn revoke(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Revoke>,
) -> Result<Json<AccountMeshOutcome<RevokedResultView>>, Error> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account_mesh_revoke(input.target)
            .await
            .map_err(unavailable)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledge {
    request_id: String,
}

/// The plans modal consumed this automatic `mesh.machines` notice.
pub async fn acknowledge_limit(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Acknowledge>,
) -> Result<Json<serde_json::Value>, Error> {
    admit(caller.as_ref())?;
    let acknowledged = state
        .svc
        .account_mesh_acknowledge_limit(&input.request_id)
        .await
        .map_err(unavailable)?;
    Ok(Json(serde_json::json!({ "acknowledged": acknowledged })))
}
