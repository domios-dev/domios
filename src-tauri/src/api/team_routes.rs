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

#[derive(Deserialize, Default)]
pub(crate) struct CloseTeamBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    confirm: Option<String>,
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
    } else if error.contains("not authorized")
        || error.contains("not a member")
        || error.contains("Unattributed caller")
        || error.contains("cannot close Team")
    {
        (StatusCode::FORBIDDEN, error)
    } else {
        (StatusCode::BAD_REQUEST, error)
    }
}

fn require_caller_tab_id(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    crate::api::tab_routes::caller_tab_header(headers).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "X-CodeTerm-Tab-Id is required".into(),
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
    let caller_tab_id = require_caller_tab_id(headers)?;
    rename(team_id, caller_tab_id, body.name).map_err(map_rename_error)
}

pub async fn rename_team(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(body): Json<RenameTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let team = rename_team_adapter(&headers, team_id, body, |team_id, caller_tab_id, name| {
        crate::commands::mux_commands::mux_rename_team_service_core(
            &state.svc,
            team_id,
            name,
            Some(caller_tab_id),
        )
    })?;
    Ok(Json(team))
}

async fn close_team_adapter(
    headers: &HeaderMap,
    team_id: String,
    body: CloseTeamBody,
    close: impl AsyncFnOnce(
        String,
        crate::service::TeamMutationCaller,
    ) -> Result<codeterm_types::team::Team, String>,
) -> Result<codeterm_types::team::Team, (StatusCode, String)> {
    let caller = close_team_caller(headers, &team_id, &body);
    close(team_id, caller)
        .await
        .map_err(map_rename_error)
}

fn close_team_caller(
    headers: &HeaderMap,
    team_id: &str,
    body: &CloseTeamBody,
) -> crate::service::TeamMutationCaller {
    crate::api::tab_routes::caller_tab_header(headers)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .map(crate::service::TeamMutationCaller::AgentTab)
        .unwrap_or_else(|| {
            if body.force && body.confirm.as_deref() == Some(team_id) {
                crate::service::TeamMutationCaller::HumanConfirmedHeaderless
            } else {
                crate::service::TeamMutationCaller::Unattributed
            }
        })
}

pub async fn close_team(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    crate::api::optional_json::OptionalJson(body): crate::api::optional_json::OptionalJson<CloseTeamBody>,
) -> Result<Json<codeterm_types::team::Team>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let team = close_team_adapter(
        &headers,
        team_id,
        body.unwrap_or_default(),
        |team_id, caller| async { state.svc.close_team(&team_id, caller).await },
    )
    .await?;
    Ok(Json(team))
}

/// The caller tab is optional here alone: a Team whose Orchestrator is gone has
/// no membership left to spend, and staffing authorizes the caller-less add.
async fn add_team_member_adapter<T>(
    headers: &HeaderMap,
    team_id: String,
    body: AddTeamMemberBody,
    add_member: impl AsyncFnOnce(String, Option<String>, AddTeamMemberBody) -> Result<T, String>,
) -> Result<T, (StatusCode, String)> {
    let caller_tab_id = crate::api::tab_routes::caller_tab_header(headers)
        .filter(|tab_id| !tab_id.trim().is_empty());
    add_member(team_id, caller_tab_id, body)
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
        |team_id, caller_tab_id, body: AddTeamMemberBody| async {
            crate::commands::mux_commands::mux_add_team_member_service_core(
                &state.svc,
                team_id,
                body.provider,
                body.role,
                caller_tab_id,
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
        let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
        let worker = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
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
            map_rename_error("Caller Tab worker is not authorized to manage Team team_x".into()).0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(validate_name("   ").unwrap_err().0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn team_rename_requires_caller_tab_header() {
        let error = require_caller_tab_id(&HeaderMap::new()).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1, "X-CodeTerm-Tab-Id is required");
    }

    #[test]
    fn http_team_rename_returns_updated_team() {
        let (mut mux, team_id, orchestrator, _) = team_members();
        let events = crate::api::events::EventBroadcaster::new();
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Tab-Id", orchestrator.parse().unwrap());

        let team = rename_team_adapter(
            &headers,
            team_id.clone(),
            RenameTeamBody {
                name: "Renamed".into(),
            },
            |team_id, caller_tab_id, name| {
                crate::service::rename_team_with_state_change(
                    &mut mux,
                    &events,
                    &team_id,
                    name,
                    &crate::service::TeamMutationCaller::AgentTab(caller_tab_id),
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
        let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
        let foreign = mux.create_tab(&workspace_id).unwrap();
        let events = crate::api::events::EventBroadcaster::new();

        let mut worker_headers = HeaderMap::new();
        worker_headers.insert("X-CodeTerm-Tab-Id", worker.parse().unwrap());
        assert_eq!(
            rename_team_adapter(
                &worker_headers,
                team_id.clone(),
                RenameTeamBody {
                    name: "Denied worker".into(),
                },
                |team_id, caller_tab_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        &crate::service::TeamMutationCaller::AgentTab(caller_tab_id),
                    )
                },
            )
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let mut foreign_headers = HeaderMap::new();
        foreign_headers.insert("X-CodeTerm-Tab-Id", foreign.parse().unwrap());
        assert_eq!(
            rename_team_adapter(
                &foreign_headers,
                team_id.clone(),
                RenameTeamBody {
                    name: "Denied foreign".into(),
                },
                |team_id, caller_tab_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        &crate::service::TeamMutationCaller::AgentTab(caller_tab_id),
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
                |team_id, caller_tab_id, name| {
                    crate::service::rename_team_with_state_change(
                        &mut mux,
                        &events,
                        &team_id,
                        name,
                        &crate::service::TeamMutationCaller::AgentTab(caller_tab_id),
                    )
                },
            )
            .unwrap_err()
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(mux.get_team(&team_id).unwrap().name, None);
    }

    #[tokio::test]
    async fn http_close_team_enforces_permission() {
        let (mut mux, team_id, orchestrator, worker) = team_members();
        let events = crate::api::events::EventBroadcaster::new();
        let plan_root = tempfile::tempdir().unwrap();
        let plan_store = crate::service::plan::PlanStore::new_for_test(plan_root.path());
        assert_eq!(
            close_team_adapter(
                &HeaderMap::new(),
                team_id.clone(),
                CloseTeamBody::default(),
                async |team_id, caller| {
                    crate::service::close_team_with_state_change(
                        &mut mux,
                        &events,
                        &plan_store,
                        &team_id,
                        &caller,
                        1,
                    )
                    .map(|result| result.0)
                }
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let mut worker_headers = HeaderMap::new();
        worker_headers.insert("X-CodeTerm-Tab-Id", worker.parse().unwrap());
        assert_eq!(
            close_team_adapter(
                &worker_headers,
                team_id.clone(),
                CloseTeamBody::default(),
                async |team_id: String, caller| {
                    crate::service::close_team_with_state_change(
                        &mut mux,
                        &events,
                        &plan_store,
                        &team_id,
                        &caller,
                        1,
                    )
                    .map(|result| result.0)
                }
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
        let foreign = mux.create_tab(&workspace_id).unwrap();
        let mut foreign_headers = HeaderMap::new();
        foreign_headers.insert("X-CodeTerm-Tab-Id", foreign.parse().unwrap());
        assert_eq!(
            close_team_adapter(
                &foreign_headers,
                team_id.clone(),
                CloseTeamBody::default(),
                async |team_id: String, caller| {
                    crate::service::close_team_with_state_change(
                        &mut mux,
                        &events,
                        &plan_store,
                        &team_id,
                        &caller,
                        1,
                    )
                    .map(|result| result.0)
                }
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let mut manager_headers = HeaderMap::new();
        manager_headers.insert("X-CodeTerm-Tab-Id", orchestrator.parse().unwrap());
        assert_eq!(
            close_team_adapter(
                &manager_headers,
                team_id.clone(),
                CloseTeamBody::default(),
                async |team_id: String, caller| {
                    crate::service::close_team_with_state_change(
                        &mut mux,
                        &events,
                        &plan_store,
                        &team_id,
                        &caller,
                        1,
                    )
                    .map(|result| result.0)
                }
            )
            .await
            .unwrap()
            .id,
            team_id
        );
    }

    #[test]
    fn headerless_team_close_requires_explicit_confirmation() {
        use crate::service::TeamMutationCaller;

        let team_id = "team-1";
        assert!(matches!(
            close_team_caller(&HeaderMap::new(), team_id, &CloseTeamBody::default()),
            TeamMutationCaller::Unattributed
        ));
        assert!(matches!(
            close_team_caller(
                &HeaderMap::new(),
                team_id,
                &CloseTeamBody {
                    force: false,
                    confirm: Some(team_id.into()),
                },
            ),
            TeamMutationCaller::Unattributed
        ));
        assert!(matches!(
            close_team_caller(
                &HeaderMap::new(),
                team_id,
                &CloseTeamBody {
                    force: true,
                    confirm: Some("team-other".into()),
                },
            ),
            TeamMutationCaller::Unattributed
        ));
        let confirmed = close_team_caller(
            &HeaderMap::new(),
            team_id,
            &CloseTeamBody {
                force: true,
                confirm: Some(team_id.into()),
            },
        );
        assert_eq!(
            confirmed,
            TeamMutationCaller::HumanConfirmedHeaderless,
        );
        assert_eq!(
            confirmed.kind(),
            "human-attributed (headerless, confirmed)",
        );
    }

    /// codeterm#418: the recovery add is the one Team verb a headless Team can
    /// still reach, so an absent caller header travels as "no caller".
    #[tokio::test]
    async fn http_add_team_member_passes_an_absent_caller_tab_through() {
        let caller = add_team_member_adapter(
            &HeaderMap::new(),
            "team_x".into(),
            AddTeamMemberBody {
                provider: "provider".into(),
                role: "orchestrator".into(),
                title: None,
                task: None,
                model: None,
            },
            |_, caller_tab_id, _| async move { Ok(caller_tab_id) },
        )
        .await
        .expect("a caller-less add reaches the service");
        assert_eq!(caller, None);
    }

    #[tokio::test]
    async fn http_add_team_member_delegates_acting_tab_and_body() {
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Tab-Id", "manager".parse().unwrap());
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
            |team_id, caller_tab_id: Option<String>, body: AddTeamMemberBody| async move {
                assert_eq!(team_id, "team_x");
                assert_eq!(caller_tab_id.as_deref(), Some("manager"));
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

    #[tokio::test]
    async fn http_cartographer_add_keeps_the_role_kind_in_the_staffing_contract() {
        let mut headers = HeaderMap::new();
        headers.insert("X-CodeTerm-Tab-Id", "manager".parse().unwrap());
        let result = add_team_member_adapter(
            &headers,
            "team_x".into(),
            AddTeamMemberBody {
                provider: "provider-neutral".into(),
                role: "cartographer".into(),
                title: None,
                task: None,
                model: None,
            },
            |team_id, caller_tab_id: Option<String>, body: AddTeamMemberBody| async move {
                assert_eq!(team_id, "team_x");
                assert_eq!(caller_tab_id.as_deref(), Some("manager"));
                Ok((body.role, body.provider))
            },
        )
        .await
        .unwrap();

        assert_eq!(result.0, "cartographer");
        assert_eq!(result.1, "provider-neutral");
    }
}
