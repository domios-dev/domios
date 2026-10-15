use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use tauri::Manager;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::api_support::OptionalJson;

#[derive(Deserialize)]
pub struct SaveSessionBody {
    pub continue_tab: Option<String>,
    pub continue_prompt: Option<String>,
    pub suppress_continue_tab: Option<String>,
    pub withdraw_continue_tab: Option<String>,
    pub withdraw_continue_prompt: Option<String>,
}

/// Wait for the current desk snapshot to commit before confirming the save.
/// Restart-persistence tests call this before restarting the daemon.
/// A `continue_tab` asks the next start to deliver one continue to that tab.
pub async fn save_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<SaveSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let body = body.unwrap_or(SaveSessionBody {
        continue_tab: None,
        continue_prompt: None,
        suppress_continue_tab: None,
        withdraw_continue_tab: None,
        withdraw_continue_prompt: None,
    });
    let actions = body.continue_tab.is_some() as usize
        + body.suppress_continue_tab.is_some() as usize
        + body.withdraw_continue_tab.is_some() as usize;
    if actions > 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "session save accepts one continuation action".to_string(),
        ));
    }
    let (continuation, suppression, withdrawal) = tokio::task::spawn_blocking(move || {
        let caller = crate::tab_routes::caller_tab_header(&headers);
        let continuation = body.continue_tab.map(|tab_id| {
            let Some(caller_tab_id) = caller.as_deref() else {
                return serde_json::json!({
                    "tab_id": tab_id,
                    "registered": false,
                    "reason": codeterm_service_core::service::turn_resume::CALLER_TAB_REQUIRED,
                });
            };
            let prompt = body
                .continue_prompt
                .filter(|prompt| !prompt.trim().is_empty())
                .unwrap_or_else(|| {
                    codeterm_service_core::service::turn_resume::DEFAULT_CONTINUE.to_string()
                });
            match state
                .svc
                .request_turn_resume_for_caller(caller_tab_id, &tab_id, &prompt)
            {
                Ok(()) => serde_json::json!({"tab_id": tab_id, "registered": true}),
                Err(reason) => {
                    serde_json::json!({"tab_id": tab_id, "registered": false, "reason": reason})
                }
            }
        });
        let suppression = body.suppress_continue_tab.map(|tab_id| {
            let result = match caller.as_deref() {
            Some(caller_tab_id) if caller_tab_id == tab_id => state
                .svc
                .suppress_turn_resume_for_caller(caller_tab_id)
                .map(|suppressed| serde_json::json!({"tab_id": tab_id, "suppressed": suppressed})),
            Some(_) => Err(codeterm_service_core::service::turn_resume::NOT_CALLER_TAB.into()),
            None => Err(codeterm_service_core::service::turn_resume::CALLER_TAB_REQUIRED.into()),
        };
            result.unwrap_or_else(
            |reason| serde_json::json!({"tab_id": tab_id, "suppressed": false, "reason": reason}),
        )
        });
        let withdrawal =
            body.withdraw_continue_tab.map(|tab_id| {
                let prompt = body.withdraw_continue_prompt.as_deref().unwrap_or_default();
                let result = match caller.as_deref() {
            Some(caller_tab_id) => state
                .svc
                .withdraw_turn_resume_request(caller_tab_id, &tab_id, prompt)
                .map(|withdrawn| serde_json::json!({"tab_id": tab_id, "withdrawn": withdrawn})),
            None => Err(codeterm_service_core::service::turn_resume::CALLER_TAB_REQUIRED.into()),
        };
                result.unwrap_or_else(
            |reason| serde_json::json!({"tab_id": tab_id, "withdrawn": false, "reason": reason}),
        )
            });
        codeterm_commands::update::shutdown::persist_session(&state.svc.app_handle, &state.svc)?;
        Ok::<_, String>((continuation, suppression, withdrawal))
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("session persistence task failed: {error}"),
        )
    })?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "continuation": continuation,
        "suppression": suppression,
        "withdrawal": withdrawal,
    })))
}

/// Rotated full-desk snapshots under `sessions/backups/`, newest first.
/// Disk-bound (reads + parses every backup), so it runs on the blocking pool.
pub async fn list_session_backups_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let backups = tokio::task::spawn_blocking(move || svc.list_session_backups())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("backup list task failed: {e}"),
            )
        })?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "backups": backups })))
}

/// Additive desk recovery from one backup: recreates workspaces/tabs missing
/// from the live desk. Never replaces or closes anything that exists.
pub async fn restore_session_backup_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let restored_name = name.clone();
    let report = tokio::task::spawn_blocking(move || svc.restore_session_backup(&name))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("backup restore task failed: {e}"),
            )
        })?
        .map_err(|e| {
            let status = if e.contains("invalid backup name") || e.contains("read backup") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, e)
        })?;
    Ok(Json(serde_json::json!({
        "restored": restored_name,
        "workspaces_added": report.workspaces_added,
        "tabs_added": report.tabs_added,
        "tabs_already_present": report.tabs_already_present,
    })))
}

#[derive(Deserialize)]
pub struct SearchSessionsQuery {
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

/// Cross-provider session search: returns matches ranked by recency plus a
/// warning per provider store that could not be fully searched. Each hit
/// carries its `provider` so callers can route a resume to the right launcher.
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
    let opts = codeterm_service_core::agent::registry::SessionSearchOpts {
        terms,
        match_mode: codeterm_service_core::agent::registry::SessionMatchMode::AllTerms,
        provider: q.provider.filter(|p| p != "all" && !p.is_empty()),
        project_path: q.project,
        limit: q.limit.unwrap_or(10),
        max_snippets: q.max_snippets.unwrap_or(3),
        max_file_bytes: 0,
    };
    let registry = state.svc.agent_registry.clone();
    let result = tokio::task::spawn_blocking(move || registry.search_sessions(&opts))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

/// Exact-id lookup still reads provider stores (files, sqlite, plugin VMs), so it
/// stays on the blocking pool rather than the async runtime.
async fn find_session_off_runtime(
    state: &AppState,
    session_id: &str,
) -> Result<Option<codeterm_service_core::agent::provider::SessionMetadata>, (StatusCode, String)> {
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
pub struct ResumeSessionBody {
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
    let storage = state
        .svc
        .app_handle
        .try_state::<codeterm_service_core::startup::StorageAuthority>()?
        .inner()
        .storage()?
        .clone();
    let adapter = codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage);
    let provider = provider.to_string();
    let session_id = session_id.to_string();
    let persisted = tokio::task::spawn_blocking(move || {
        adapter
            .identity_catalog()
            .ok()?
            .into_iter()
            .find(|identity| identity.provider == provider && identity.session_id == session_id)
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
    OptionalJson(body): OptionalJson<ResumeSessionBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.unwrap_or(ResumeSessionBody {
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
        codeterm_service_core::service::permission_policy_configured_permission_bypass_for(
            &state.svc.app_handle,
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
    let owner = state.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        let state = owner;
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
        let mux = state.svc.mux.read();
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
            return Ok(Err(Json(serde_json::json!({
                "tab_id": tab_id,
                "workspace_id": ws_id,
                "provider": meta.provider,
                "session_id": meta.session_id,
                "cwd": cwd,
                "resume_command": resume_cmd_preview,
                "identity": live_identity,
            }))));
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
                let cmd = codeterm_service_core::dispatch::Command::WorkspaceCreate {
                    name: ws_name,
                    cwd: Some(cwd.clone()),
                    activate: Some(true),
                    provision: false,
                };
                let result = codeterm_service_core::dispatch::dispatch(&cmd, state.svc.as_ref())
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

    let spawn_params = codeterm_service_core::service::SpawnAgentParams {
        agent: Some(meta.provider.clone()),
        remote_task_binding: None,
        context_files: None,
        context_skills: None,
        fork_session: false,
        agent_name: None,
        args: None,
        cwd: Some(cwd.clone()),
        provisioned_cwd: None,
        title: body.title.or_else(|| {
            meta.title
                .clone()
                .or_else(|| Some(format!("resumed {}", &meta.session_id[..8])))
        }),
        title_provenance: Some(
            codeterm_service_core::service::SpawnIdentityProvenance::UserSupplied,
        ),
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
        starter_prompt: codeterm_service_core::agent::provider::StarterPrompt::NoStarter,
        task_id: None,
        task_launch_authority: None,
        plan_skill_manifest: None,
        plan_task: None,
        authorized_gates: Vec::new(),
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
        team_id: None,
        creator_tab_id: None,
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
            user_created: false,
        structured_prompt_server: None,
    };
        Ok::<_, (StatusCode, String)>(Ok((spawn_params, identity, ws_id, meta, cwd, resume_cmd_preview)))
    }).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed".to_string()))??;
    let (spawn_params, identity, ws_id, meta, cwd, resume_cmd_preview) = match prepared {
        Ok(prepared) => prepared,
        Err(response) => return Ok(response),
    };
    let spawned = state
        .svc
        .spawn_agent(spawn_params, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    tokio::task::spawn_blocking(move || {
        state.svc.set_and_lock_tab_agent_type_from(
            &spawned.tab_id,
            meta.provider.clone(),
            codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Spawn,
            None,
        );
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
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
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
        let params = codeterm_service_core::service::SpawnAgentParams {
            role: Some("Worker".into()),
            parent_tab_id: Some("orchestrator".into()),
            can_spawn: Some(false),
            task: None,
            starter_prompt: codeterm_service_core::agent::provider::StarterPrompt::NoStarter,
            task_id: None,
            plan_task: None,
            resume_session_id: Some("owned-session".into()),
            ..Default::default()
        };
        assert_eq!(params.task, None);
        assert_eq!(
            params.starter_prompt,
            codeterm_service_core::agent::provider::StarterPrompt::NoStarter
        );
        assert_eq!(params.task_id, None);
        assert_eq!(params.plan_task, None);
        let restored_context =
            codeterm_service_core::service::spawn_onboarding_task(&params).unwrap();
        assert!(!restored_context.contains("historical"));
        assert!(!restored_context.contains("one assigned brief"));

        let fresh = codeterm_service_core::service::SpawnAgentParams {
            role: Some("Worker".into()),
            task: Some("one assigned brief".into()),
            starter_prompt: codeterm_service_core::agent::provider::StarterPrompt::AssignedTask,
            ..Default::default()
        };
        let fresh_context = codeterm_service_core::service::spawn_onboarding_task(&fresh).unwrap();
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
