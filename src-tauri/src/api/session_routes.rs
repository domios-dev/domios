use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use tauri::Manager;

use crate::api::routes::{check_auth, AppState};

/// Snapshot the multiplexer into the session store NOW. Durability is
/// otherwise the 30s auto-save timer plus the close-to-tray hook — a
/// force-killed daemon (taskkill, crash, e2e harness) loses anything
/// younger than the last tick. Restart-persistence tests call this first.
pub async fn save_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    crate::update::shutdown::persist_session(&state.svc.app_handle, &state.svc)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub(crate) struct SearchSessionsQuery {
    q: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    max_snippets: Option<usize>,
}

/// Cross-provider session search: walks every registered `AgentProvider`,
/// collects past sessions from disk, and returns matches ranked by recency.
/// Each hit carries its `provider` so callers can route a "resume in new Tab"
/// action to the correct launcher.
///
/// The scan is disk-bound, so it runs on the blocking pool — on the async
/// runtime it starves every other IPC caller for the length of the corpus walk.
pub async fn search_sessions_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<SearchSessionsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let terms: Vec<String> =
        q.q.split_whitespace()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
    if terms.is_empty() {
        return Ok(Json(serde_json::json!({ "hits": [] })));
    }
    let opts = crate::agent::registry::SessionSearchOpts {
        terms,
        provider: q.provider.filter(|p| p != "all" && !p.is_empty()),
        project_path: q.project,
        limit: q.limit.unwrap_or(10),
        max_snippets: q.max_snippets.unwrap_or(3),
        max_file_bytes: 0,
    };
    let registry = state.svc.agent_registry.clone();
    let hits = tokio::task::spawn_blocking(move || registry.search_sessions(&opts))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(serde_json::json!({ "hits": hits })))
}

/// Session lookup enumerates every provider's on-disk history, so it belongs on
/// the blocking pool for the same reason `search_sessions_api` does.
async fn find_session_off_runtime(
    state: &AppState,
    session_id: &str,
) -> Result<Option<crate::agent::provider::SessionMetadata>, (StatusCode, String)> {
    let registry = state.svc.agent_registry.clone();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || registry.find_session(&session_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))
}

/// Resolve a session id to its metadata across all providers.
/// Returns `null` + 404 when nothing claims the id.
pub async fn get_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    match find_session_off_runtime(&state, &session_id).await? {
        Some(meta) => Ok(Json(serde_json::to_value(meta).unwrap_or_default())),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("session {} not found", session_id),
        )),
    }
}

#[derive(Deserialize)]
pub(crate) struct ResumeSessionBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResumableIdentity {
    role: String,
    role_profile: Option<String>,
    parent_tab_id: Option<String>,
    can_spawn: bool,
}

async fn persisted_resume_identity(
    state: &AppState,
    provider: &str,
    session_id: &str,
) -> Option<ResumableIdentity> {
    let session = state
        .svc
        .app_handle
        .try_state::<crate::session::SessionState>()?
        .inner()
        .clone();
    let provider = provider.to_string();
    let session_id = session_id.to_string();
    let persisted = tokio::task::spawn_blocking(move || {
        let store = session.lock();
        store.load_agent_session_identity(&provider, &session_id)
    })
    .await
    .ok()??;
    Some(ResumableIdentity {
        role: persisted.role,
        role_profile: persisted.role_profile,
        parent_tab_id: persisted.parent_tab_id,
        can_spawn: persisted.can_spawn,
    })
}

fn restorable_identity(
    identity: Option<ResumableIdentity>,
    parent_is_live_orchestrator: impl Fn(&str) -> bool,
) -> Option<ResumableIdentity> {
    let identity = identity?;
    let restored = codeterm_types::agent::restore_role(
        Some(identity.role.as_str()),
        identity.role_profile.as_deref(),
    )?;
    // An anchor stands alone; every other kind needs its parent still live.
    if codeterm_types::agent::role_kind::anchors_team(
        Some(restored.role),
        restored.role_profile.as_deref(),
    ) {
        return identity.parent_tab_id.is_none().then_some(identity);
    }
    match identity.parent_tab_id.as_deref() {
        Some(parent) => parent_is_live_orchestrator(parent).then_some(identity),
        None => None,
    }
}

fn roleless_actions(tab_id: &str) -> serde_json::Value {
    serde_json::json!({
        "promotion": format!("codeterm agent set-orchestrator --tab {tab_id}"),
        "relink": format!("codeterm agent link --parent-tab <orchestrator-tab> --tab {tab_id}"),
    })
}

/// Atomic "resume a past session" action:
///   1. locate the session across providers → get cwd + provider
///   2. create (or pick) a workspace whose cwd matches that project path
///   3. create a tab + PTY in that workspace
///   4. wait for shell prompt + send the provider's resume command
pub async fn resume_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Option<Json<ResumeSessionBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.map(|Json(v)| v).unwrap_or(ResumeSessionBody {
        title: None,
        workspace_id: None,
        workspace_name: None,
        model: None,
    });

    let meta = find_session_off_runtime(&state, &session_id)
        .await?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("session {} not found", session_id),
        ))?;

    let cwd = meta.project_path.clone().ok_or((
        StatusCode::BAD_REQUEST,
        format!(
            "session {} has no recorded project_path; can't decide cwd",
            session_id
        ),
    ))?;

    let permission_bypass_authorization =
        crate::service::permission_policy::configured_permission_bypass(
            &state.svc.agent_registry,
            &meta.provider,
        )
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let permission_bypass = permission_bypass_authorization.is_some();
    let resume_cmd_preview = state
        .svc
        .agent_registry
        .with_provider(&meta.provider, |p| {
            p.build_resume_command(&meta.session_id, permission_bypass)
        })
        .flatten();

    let persisted_identity =
        persisted_resume_identity(&state, &meta.provider, &meta.session_id).await;
    let identity = restorable_identity(persisted_identity, |parent| {
        state
            .svc
            .mux
            .read()
            .get_tab(parent)
            .is_some_and(|tab| tab.anchors_team())
    });

    // both spawn.
    {
        let mux = state.svc.mux.write();
        let existing_tab = mux.tab_ids_with_session(&session_id).into_iter().next();
        if let Some(tab_id) = existing_tab {
            let snap = mux.snapshot();
            let ws_id = snap
                .workspaces
                .iter()
                .find_map(|w| w.tabs.contains(&tab_id).then(|| w.id.clone()))
                .unwrap_or_default();
            let live_identity = mux
                .get_tab(&tab_id)
                .and_then(|tab| {
                    let binding_is_authoritative = tab.session_binding_nonce.is_some()
                        && tab.session_binding_nonce == tab.reported_session_binding_nonce;
                    let role = binding_is_authoritative.then_some(tab)?.role?;
                    Some(serde_json::json!({
                        "status": "restored",
                        "role": format!("{role:?}"),
                        "role_profile": tab.role_profile.clone(),
                        "parent_tab_id": tab.parent_tab_id.clone(),
                        "can_spawn": tab.can_spawn,
                    }))
                })
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "status": "roleless",
                        "role": null,
                        "role_profile": null,
                        "parent_tab_id": null,
                        "can_spawn": false,
                        "actions": roleless_actions(&tab_id),
                    })
                });
            return Ok(Json(serde_json::json!({
                "tab_id": tab_id,
                "workspace_id": ws_id,
                "provider": meta.provider,
                "session_id": meta.session_id,
                "cwd": cwd,
                "resume_command": resume_cmd_preview,
                "identity": live_identity,
            })));
        }
    }

    let ws_id = match body.workspace_id {
        Some(id) => id,
        None => {
            // Ignore extra effect for programmatic resume; it should only create the single resumed agent tab.
            let existing_id = state
                .svc
                .mux
                .write()
                .find_workspace_info_by_cwd(&cwd)
                .map(|w| w.id);
            if let Some(id) = existing_id {
                if let Err(e) = state.svc.mux.write().set_active_workspace(&id) {
                    log::warn!("[session_resume] set_active_workspace({id}) failed: {e}; workspace may have been concurrently closed");
                }
                state.svc.emit_state_changed();
                id
            } else {
                let ws_name = body.workspace_name.unwrap_or_else(|| {
                    std::path::Path::new(&cwd)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&meta.provider)
                        .to_string()
                });
                let cmd = crate::dispatch::Command::WorkspaceCreate {
                    name: ws_name,
                    cwd: Some(cwd.clone()),
                    activate: Some(true),
                };
                let result = crate::dispatch::dispatch(&cmd, state.svc.as_ref())
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
                result
                    .into_json()
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .ok_or((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "workspace create returned no id".into(),
                    ))?
            }
        }
    };

    let spawn_params = crate::service::SpawnAgentParams {
        agent: Some(meta.provider.clone()),
        agent_name: None,
        args: None,
        cwd: Some(cwd.clone()),
        provisioned_cwd: None,
        title: body.title.or_else(|| {
            meta.title
                .clone()
                .or_else(|| Some(format!("resumed {}", &meta.session_id[..8])))
        }),
        title_provenance: Some(crate::service::SpawnIdentityProvenance::UserSupplied),
        description: None,
        role: identity.as_ref().map(|identity| identity.role.clone()),
        role_profile: identity
            .as_ref()
            .and_then(|identity| identity.role_profile.clone()),
        parent_tab_id: identity
            .as_ref()
            .and_then(|identity| identity.parent_tab_id.clone()),
        plan_id: None,
        can_spawn: identity.as_ref().map(|identity| identity.can_spawn),
        task: None,
        starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
        task_id: None,
        task_launch_authority: None,
        plan_skill_manifest: None,
        plan_task: None,
        execution_intent: None,
        provider_reserve_pct: None,
        workspace_id: Some(ws_id.clone()),
        force_tab: None,
        existing_tab_id: None,
        skip_orchestrator_onboarding: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        skip_permissions: Some(permission_bypass),
        permission_bypass_authorization,
        spawned_by_tab_id: None,
        ownership: None,
        model: body.model.clone(),
        planned_model: None,
        planned_reasoning_effort: None,
        reasoning_effort: None,
        system_prompt: None,
        preset: None,
        resume_session_id: Some(meta.session_id.clone()),
        charter: None,
        tick_interval_secs: None,
        nudge_cooldown_secs: None,
        max_actions_per_tick: None,
        activate: None,
        audience: Default::default(),
        manager_bootstrap: None,
        structured_prompt_server: None,
    };
    let spawned = state
        .svc
        .spawn_agent(spawn_params, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state
        .svc
        .set_and_lock_tab_agent_type(&spawned.tab_id, meta.provider.clone());
    state.svc.emit_state_changed();

    let identity_json = match identity {
        Some(identity) => serde_json::json!({
            "status": "restored",
            "role": identity.role,
            "role_profile": identity.role_profile,
            "parent_tab_id": identity.parent_tab_id,
            "can_spawn": identity.can_spawn,
        }),
        None => serde_json::json!({
            "status": "roleless",
            "role": null,
            "role_profile": null,
            "parent_tab_id": null,
            "can_spawn": false,
            "actions": roleless_actions(&spawned.tab_id),
        }),
    };

    Ok(Json(serde_json::json!({
        "tab_id": spawned.tab_id,
        "workspace_id": ws_id,
        "provider": meta.provider,
        "session_id": meta.session_id,
        "cwd": cwd,
        "resume_command": resume_cmd_preview,
        "identity": identity_json,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_resume_restores_role_without_replaying_starter() {
        let worker = ResumableIdentity {
            role: "Worker".into(),
            role_profile: None,
            parent_tab_id: Some("parent".into()),
            can_spawn: false,
        };
        assert!(restorable_identity(Some(worker.clone()), |parent| parent == "parent").is_some());
        assert!(restorable_identity(Some(worker), |_| false).is_none());

        let orchestrator = ResumableIdentity {
            role: "Orchestrator".into(),
            role_profile: None,
            parent_tab_id: None,
            can_spawn: true,
        };
        assert!(restorable_identity(Some(orchestrator), |_| false).is_some());
    }

    #[test]
    fn restored_worker_identity_has_no_historical_task_input() {
        let params = crate::service::SpawnAgentParams {
            role: Some("Worker".into()),
            parent_tab_id: Some("orchestrator".into()),
            can_spawn: Some(false),
            task: None,
            starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
            task_id: None,
            plan_task: None,
            resume_session_id: Some("owned-session".into()),
            ..Default::default()
        };
        assert_eq!(params.task, None);
        assert_eq!(
            params.starter_prompt,
            crate::agent::provider::StarterPrompt::NoStarter
        );
        assert_eq!(params.task_id, None);
        assert_eq!(params.plan_task, None);
        let restored_context = crate::service::spawn_onboarding_task(&params).unwrap();
        assert!(!restored_context.contains("historical"));
        assert!(!restored_context.contains("one assigned brief"));

        let fresh = crate::service::SpawnAgentParams {
            role: Some("Worker".into()),
            task: Some("one assigned brief".into()),
            starter_prompt: crate::agent::provider::StarterPrompt::AssignedTask,
            ..Default::default()
        };
        let fresh_context = crate::service::spawn_onboarding_task(&fresh).unwrap();
        assert_eq!(fresh_context.matches("one assigned brief").count(), 1);
    }

    #[test]
    fn roleless_resume_reports_exact_promotion_and_relink_actions() {
        let actions = roleless_actions("tab-resumed");
        assert_eq!(
            actions["promotion"],
            "codeterm agent set-orchestrator --tab tab-resumed"
        );
        assert_eq!(
            actions["relink"],
            "codeterm agent link --parent-tab <orchestrator-tab> --tab tab-resumed"
        );
    }
}
