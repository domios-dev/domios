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

#[derive(Deserialize)]
pub(crate) struct AddTeamMemberBody {
    provider: String,
    role: String,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
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

fn rename_team_adapter(
    headers: &HeaderMap,
    team_id: String,
    body: RenameTeamBody,
    rename: impl FnOnce(String, String, String) -> Result<codeterm_types::team::Team, String>,
) -> Result<codeterm_types::team::Team, (StatusCode, String)> {
    validate_name(&body.name)?;
    let caller_pane_id = require_caller_pane_id(headers)?;
    rename(team_id, caller_pane_id, body.name).map_err(map_rename_error)
}

pub async fn rename_team(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<RenameTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let team = rename_team_adapter(&headers, team_id, body, |team_id, caller_pane_id, name| {
        crate::commands::mux_commands::mux_rename_team_service_core(
            &state.svc,
            team_id,
            name,
            Some(caller_pane_id),
        )
    })?;
    Ok(Json(team))
}

fn close_team_adapter(
    headers: &HeaderMap,
    team_id: String,
    close: impl FnOnce(String, String) -> Result<codeterm_types::team::Team, String>,
) -> Result<codeterm_types::team::Team, (StatusCode, String)> {
    let caller_pane_id = require_caller_pane_id(headers)?;
    close(team_id, caller_pane_id).map_err(map_rename_error)
}

pub async fn close_team(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let team = close_team_adapter(&headers, team_id, |team_id, caller_pane_id| {
        crate::commands::mux_commands::mux_close_team_service_core(
            &state.svc,
            team_id,
            Some(caller_pane_id),
        )
    })?;
    Ok(Json(team))
}

async fn add_team_member_adapter<T>(
    headers: &HeaderMap,
    team_id: String,
    body: AddTeamMemberBody,
    add_member: impl AsyncFnOnce(String, String, AddTeamMemberBody) -> Result<T, String>,
) -> Result<T, (StatusCode, String)> {
    let caller_pane_id = require_caller_pane_id(headers)?;
    add_member(team_id, caller_pane_id, body)
        .await
        .map_err(map_rename_error)
}

pub async fn add_team_member(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<AddTeamMemberBody>,
) -> Result<Json<crate::service::AddTeamMemberResult>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let result = add_team_member_adapter(
        &headers,
        team_id,
        body,
        |team_id, caller_pane_id, body: AddTeamMemberBody| async {
            crate::commands::mux_commands::mux_add_team_member_service_core(
                &state.svc,
                team_id,
                body.provider,
                body.role,
                Some(caller_pane_id),
                body.title,
                body.task,
                body.model,
            )
            .await
        },
    )
    .await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;

    fn team_members() -> (MultiplexerState, String, String, String) {
        let (mut mux, orchestrator) = MultiplexerState::new();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        let workspace_id = mux.find_workspace_for_pane(&orchestrator).unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap().1;
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orchestrator))
            .unwrap();
        (mux, team_id, orchestrator, worker)
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

    #[test]
    fn http_team_rename_returns_updated_team() {
        let (mut mux, team_id, orchestrator, _) = team_members();
        let events = crate::api::events::EventBroadcaster::new();
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Pane-Id", orchestrator.parse().unwrap());

        let team = rename_team_adapter(
            &headers,
            team_id.clone(),
            RenameTeamBody {
                name: "Renamed".into(),
            },
            |team_id, caller_pane_id, name| {
                crate::service::rename_team_with_state_change(
                    &mut mux,
                    &events,
                    &team_id,
                    name,
                    Some(&caller_pane_id),
                )
            },
        )
        .unwrap();
        assert_eq!(team.id, team_id);
        assert_eq!(team.name.as_deref(), Some("Renamed"));
    }

    #[test]
    fn http_team_rename_enforces_caller_permission() {
        let (mut mux, team_id, _orchestrator, worker) = team_members();
        let foreign = mux.create_pane();
        let events = crate::api::events::EventBroadcaster::new();

        let mut worker_headers = HeaderMap::new();
        worker_headers.insert("X-CodeTerm-Pane-Id", worker.parse().unwrap());
        assert_eq!(
            rename_team_adapter(
                &worker_headers,
                team_id.clone(),
                RenameTeamBody {
                    name: "Denied worker".into(),
                },
                |team_id, caller_pane_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        Some(&caller_pane_id),
                    )
                },
            )
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let mut foreign_headers = HeaderMap::new();
        foreign_headers.insert("X-CodeTerm-Pane-Id", foreign.parse().unwrap());
        assert_eq!(
            rename_team_adapter(
                &foreign_headers,
                team_id.clone(),
                RenameTeamBody {
                    name: "Denied foreign".into(),
                },
                |team_id, caller_pane_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        Some(&caller_pane_id),
                    )
                },
            )
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            rename_team_adapter(
                &worker_headers,
                team_id.clone(),
                RenameTeamBody { name: "   ".into() },
                |_, _, _| unreachable!(),
            )
            .unwrap_err()
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            rename_team_adapter(
                &HeaderMap::new(),
                team_id.clone(),
                RenameTeamBody {
                    name: "Missing caller".into(),
                },
                |_, _, _| unreachable!(),
            )
            .unwrap_err()
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            rename_team_adapter(
                &worker_headers,
                "team_missing".into(),
                RenameTeamBody {
                    name: "Missing team".into(),
                },
                |team_id, caller_pane_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        Some(&caller_pane_id),
                    )
                },
            )
            .unwrap_err()
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(mux.get_team(&team_id).unwrap().name, None);
    }

    #[test]
    fn http_close_team_enforces_permission() {
        let (mut mux, team_id, orchestrator, worker) = team_members();
        let events = crate::api::events::EventBroadcaster::new();
        let plan_store = crate::service::plan::PlanStore::new();
        assert_eq!(
            close_team_adapter(&HeaderMap::new(), team_id.clone(), |_, _| unreachable!())
                .unwrap_err()
                .0,
            StatusCode::BAD_REQUEST
        );
        let mut worker_headers = HeaderMap::new();
        worker_headers.insert("X-CodeTerm-Pane-Id", worker.parse().unwrap());
        assert_eq!(
            close_team_adapter(&worker_headers, team_id.clone(), |team_id, caller| {
                crate::service::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    Some(&caller),
                    1,
                )
                .map(|result| result.0)
            })
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let foreign = mux.create_pane();
        let mut foreign_headers = HeaderMap::new();
        foreign_headers.insert("X-CodeTerm-Pane-Id", foreign.parse().unwrap());
        assert_eq!(
            close_team_adapter(&foreign_headers, team_id.clone(), |team_id, caller| {
                crate::service::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    Some(&caller),
                    1,
                )
                .map(|result| result.0)
            })
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let mut manager_headers = HeaderMap::new();
        manager_headers.insert("X-CodeTerm-Pane-Id", orchestrator.parse().unwrap());
        assert_eq!(
            close_team_adapter(&manager_headers, team_id.clone(), |team_id, caller| {
                crate::service::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    Some(&caller),
                    1,
                )
                .map(|result| result.0)
            })
            .unwrap()
            .id,
            team_id
        );
    }

    #[tokio::test]
    async fn http_add_team_member_requires_caller_pane_header() {
        let error = add_team_member_adapter(
            &HeaderMap::new(),
            "team_x".into(),
            AddTeamMemberBody {
                provider: "provider".into(),
                role: "worker".into(),
                title: None,
                task: None,
                model: None,
            },
            |_, _, _| async { Ok(()) },
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1, "X-CodeTerm-Pane-Id is required");
    }

    #[tokio::test]
    async fn http_add_team_member_delegates_acting_pane_and_body() {
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Pane-Id", "manager".parse().unwrap());
        let result = add_team_member_adapter(
            &headers,
            "team_x".into(),
            AddTeamMemberBody {
                provider: "provider".into(),
                role: "planner".into(),
                title: Some("Planner".into()),
                task: Some("Plan it".into()),
                model: Some("model".into()),
            },
            |team_id, caller_pane_id, body: AddTeamMemberBody| async move {
                assert_eq!(team_id, "team_x");
                assert_eq!(caller_pane_id, "manager");
                assert_eq!(body.role, "planner");
                Ok((body.provider, body.title, body.task, body.model))
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            (
                "provider".into(),
                Some("Planner".into()),
                Some("Plan it".into()),
                Some("model".into()),
            )
        );
    }
}
