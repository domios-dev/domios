use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct RenameTeamBody {
    name: String,
}

fn validate_name(name: &str) -> Result<(), (StatusCode, String)> {
    if name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Team name must not be empty".into(),
        ));
    }
    Ok(())
}

fn map_rename_error(error: String) -> (StatusCode, String) {
    if error.contains("not found") {
        (StatusCode::NOT_FOUND, error)
    } else if error.contains("not authorized") || error.contains("not a member") {
        (StatusCode::FORBIDDEN, error)
    } else {
        (StatusCode::BAD_REQUEST, error)
    }
}

fn require_caller_pane_id(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    crate::api::pane_routes::caller_pane_header(headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "X-CodeTerm-Pane-Id is required".into(),
        )
    })
}

pub async fn rename_team(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<RenameTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    validate_name(&body.name)?;
    let caller_pane_id = require_caller_pane_id(&headers)?;
    let team = crate::commands::mux_commands::mux_rename_team_core(
        &state.svc,
        team_id,
        body.name,
        Some(caller_pane_id),
    )
    .map_err(map_rename_error)?;
    Ok(Json(team))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_rename_errors_have_stable_http_statuses() {
        assert_eq!(
            map_rename_error("Team team_x not found".into()).0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            map_rename_error("Caller pane worker is not authorized to manage Team team_x".into()).0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(validate_name("   ").unwrap_err().0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn team_rename_requires_caller_pane_header() {
        let error = require_caller_pane_id(&HeaderMap::new()).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1, "X-CodeTerm-Pane-Id is required");
    }
}
