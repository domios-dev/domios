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
    use std::sync::atomic::Ordering;

    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::api::routes::AppState;
    use crate::multiplexer::types::AgentRole;
    use crate::service::test_support::{test_harness, TestHarness};

    fn app_state(harness: &TestHarness) -> AppState {
        AppState {
            svc: harness.service.clone(),
            cron: harness.cron.clone(),
            cron_wake: harness.cron_wake.clone(),
            api_port: 0,
            update_state: harness.update.clone(),
            config: harness.config.clone(),
            mesh: harness.mesh.clone(),
            keepawake: harness.keepawake.clone(),
            resize_owner: harness.resize_owner.clone(),
            test_clock: None,
            events: harness.events.clone(),
            web_dist: None,
        }
    }

    fn team_members(harness: &TestHarness) -> (String, String, String) {
        let mut mux = harness.service.mux.write();
        let orchestrator = mux.get_focused_pane_id().unwrap().to_string();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (team_id, orchestrator, worker)
    }

    fn harness() -> (tempfile::TempDir, TestHarness) {
        let dir = tempfile::tempdir().unwrap();
        let harness = test_harness(dir.path(), "test-provider");
        harness.config.lock().api.token = Some("team-rename-test-token".into());
        harness
            .service
            .mux_emit_stats
            .frozen
            .store(true, Ordering::Release);
        (dir, harness)
    }

    fn request(uri: String, caller_tab_id: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("Authorization", "Bearer team-rename-test-token")
            .header("Content-Type", "application/json");
        if let Some(caller_tab_id) = caller_tab_id {
            builder = builder.header("X-CodeTerm-Pane-Id", caller_tab_id);
        }
        builder.body(Body::from(r#"{"name":"Renamed"}"#)).unwrap()
    }

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

    #[tokio::test]
    async fn http_team_rename_returns_updated_team() {
        let (_dir, harness) = harness();
        let (team_id, orchestrator, _) = team_members(&harness);
        let app = crate::api::routes::create_router(app_state(&harness));

        let response = app
            .oneshot(request(
                format!("/api/teams/{team_id}/rename"),
                Some(&orchestrator),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let team: codeterm_types::team::Team = serde_json::from_slice(&body).unwrap();
        assert_eq!(team.id, team_id);
        assert_eq!(team.name.as_deref(), Some("Renamed"));
    }

    #[tokio::test]
    async fn http_team_rename_enforces_caller_permission() {
        let (_dir, harness) = harness();
        let (team_id, _orchestrator, worker) = team_members(&harness);
        let foreign = harness.service.mux.write().create_pane();
        let app = crate::api::routes::create_router(app_state(&harness));

        let worker_response = app
            .clone()
            .oneshot(request(
                format!("/api/teams/{team_id}/rename"),
                Some(&worker),
            ))
            .await
            .unwrap();
        assert_eq!(worker_response.status(), StatusCode::FORBIDDEN);

        let foreign_response = app
            .clone()
            .oneshot(request(
                format!("/api/teams/{team_id}/rename"),
                Some(&foreign),
            ))
            .await
            .unwrap();
        assert_eq!(foreign_response.status(), StatusCode::FORBIDDEN);

        let missing_caller_response = app
            .clone()
            .oneshot(request(format!("/api/teams/{team_id}/rename"), None))
            .await
            .unwrap();
        assert_eq!(missing_caller_response.status(), StatusCode::BAD_REQUEST);

        let missing_team_response = app
            .oneshot(request(
                "/api/teams/team_missing/rename".into(),
                Some(&worker),
            ))
            .await
            .unwrap();
        assert_eq!(missing_team_response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            harness.service.mux.read().get_team(&team_id).unwrap().name,
            None
        );
    }
}
