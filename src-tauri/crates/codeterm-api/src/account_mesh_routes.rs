use super::account_routes::{admit, unavailable};
use super::{
    auth::{AgentHeaders, CallerIdentity, RequestActor},
    routes::AppState,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use codeterm_account::account_mesh::wire::MeshTarget;
use codeterm_neighbors_service::prelude::*;
use codeterm_neighbors_service::service::account_mesh::{
    AccountMeshOutcome, AccountMeshRemoveOutcome, AccountMeshSnapshot, RemoveTargetError,
    RevokedResultView, UpdatedResultView,
};
use codeterm_service_core::mesh::machine_identity::MachineIdentity;
use serde::Deserialize;
type Error = (StatusCode, String);

/// Local-only Account Mesh facade.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/account/mesh", get(state))
        .route("/api/account/mesh/summary", get(summary))
        .route("/api/account/mesh/enroll", post(enroll))
        .route("/api/account/mesh/list", post(list))
        .route("/api/account/mesh/rename", post(rename_self))
        .route(
            "/api/account/mesh/identity",
            get(machine_identity).post(set_machine_identity),
        )
        .route("/api/account/mesh/revoke", post(revoke))
        .route("/api/account/mesh/remove", post(remove))
        .route("/api/account/mesh/session-revoke", post(revoke_session))
        .route(
            "/api/account/mesh/limit-notice/ack",
            post(acknowledge_limit),
        )
}

fn admit_summary(caller: Option<&Extension<CallerIdentity>>) -> Result<(), Error> {
    match caller.map(|caller| &caller.0) {
        Some(CallerIdentity::Local | CallerIdentity::Tab(_)) => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Account mesh summary requires a local caller".into(),
        )),
    }
}

#[derive(serde::Serialize)]
pub(crate) struct MachineIdentityView {
    name: String,
    identity: MachineIdentity,
    chosen: bool,
}

pub(crate) async fn machine_identity(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<MachineIdentityView>, Error> {
    admit(caller.as_ref())?;
    let manager = state.mesh.lock();
    Ok(Json(MachineIdentityView {
        name: manager.ident.instance_name.clone(),
        identity: manager.machine_identity(),
        chosen: manager.machine_identity_chosen(),
    }))
}

pub(crate) async fn set_machine_identity(
    State(state): State<AppState>,
    headers: AgentHeaders,
    Json(identity): Json<MachineIdentity>,
) -> Result<Json<serde_json::Value>, Error> {
    tokio::task::spawn_blocking(move || match headers.actor {
        RequestActor::Owner => {
            state
                .svc
                .update_machine_identity(identity, None)
                .map_err(unavailable)?;
            Ok(Json(
                serde_json::json!({ "state": "saved", "identity": identity }),
            ))
        }
        RequestActor::LocalAgent(_) | RequestActor::RemoteAgent(_) => state
            .svc
            .request_machine_identity(identity)
            .map(Json)
            .map_err(unavailable),
        _ => Err((StatusCode::FORBIDDEN, "Owner authorization required".into())),
    })
    .await
    .map_err(|error| unavailable(error.to_string()))?
}

pub async fn summary(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<AccountMeshSnapshot>, Error> {
    admit_summary(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account_mesh_snapshot()
            .await
            .map_err(unavailable)?,
    ))
}

/// `account_mesh_state`
pub async fn state(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
) -> Result<Json<AccountMeshSnapshot>, Error> {
    admit(caller.as_ref()).map_err(|(status, _)| (
        status, "Account Mesh details are owner-only. Ask the owner to run `domios mesh status` in a local terminal.".to_owned()
    ))?;
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

/// Removes an installation from the account mesh.
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
pub struct Remove {
    machine: String,
}

fn remove_target_response(error: RemoveTargetError) -> Response {
    let status = match &error {
        RemoveTargetError::Empty => StatusCode::BAD_REQUEST,
        RemoveTargetError::CannotRemoveSelf => StatusCode::FORBIDDEN,
        RemoveTargetError::NotFound(_) => StatusCode::NOT_FOUND,
        RemoveTargetError::AlreadyRemoved(_) | RemoveTargetError::Ambiguous { .. } => {
            StatusCode::CONFLICT
        }
    };
    let mut body = serde_json::json!({
        "code": error.code(),
        "error": error.message(),
    });
    if let RemoveTargetError::Ambiguous { matches, .. } = &error {
        body["matches"] = serde_json::json!(matches);
    }
    (status, Json(body)).into_response()
}

/// Resolves and removes another machine from the account mesh, signing it out first when online.
pub async fn remove(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<Remove>,
) -> Result<Response, Error> {
    admit(caller.as_ref())?;
    match state
        .svc
        .account_mesh_remove(input.machine)
        .await
        .map_err(unavailable)?
    {
        AccountMeshRemoveOutcome::Outcome(outcome) => Ok(Json(outcome).into_response()),
        AccountMeshRemoveOutcome::Target(error) => Ok(remove_target_response(error)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeSession {
    installation_id: Option<String>,
}

pub async fn revoke_session(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(input): Json<RevokeSession>,
) -> Result<
    Json<AccountMeshOutcome<codeterm_account::account_mesh::wire::SessionRevokedResult>>,
    Error,
> {
    admit(caller.as_ref())?;
    Ok(Json(
        state
            .svc
            .account_mesh_session_revoke(input.installation_id)
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

#[cfg(test)]
mod tests;
