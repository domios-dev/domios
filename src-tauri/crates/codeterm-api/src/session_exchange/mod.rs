use crate::routes::{check_auth, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use codeterm_service_core::service::MuxEmitCause;
use codeterm_service_core::session::store::{SessionSnapshot, SplitNodeSnapshot};
use serde_json::{json, Value};

mod sanitize;

type Failure = (StatusCode, String);

fn import_target_allowed(dev: bool, port: u16, suffix: &str) -> bool {
    dev && port != 7685 && suffix.starts_with("-upgrade-restore-")
}

fn failure(error: impl ToString) -> Failure {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn leaves<'a>(node: &'a SplitNodeSnapshot, result: &mut Vec<&'a SplitNodeSnapshot>) {
    match node {
        SplitNodeSnapshot::Leaf { .. } => result.push(node),
        SplitNodeSnapshot::Split { children, .. } => {
            for child in children {
                leaves(child, result);
            }
        }
    }
}

fn replay_cwd(node: &mut SplitNodeSnapshot, directory: &str) {
    match node {
        SplitNodeSnapshot::Leaf {
            cwd,
            agent_identity,
            ..
        } => {
            *cwd = Some(directory.into());
            if let Some(identity) = agent_identity {
                identity.provisioned_cwd = None;
                if let Some(options) = identity.restorable_launch.runtime_launch_options.as_mut() {
                    for additional in &mut options.additional_directories {
                        *additional = directory.into();
                    }
                }
            }
        }
        SplitNodeSnapshot::Split { children, .. } => {
            for child in children {
                replay_cwd(child, directory);
            }
        }
    }
}

fn validate_layout(node: &codeterm_storage::DeskLayoutNode) -> Result<(), String> {
    match node {
        codeterm_storage::DeskLayoutNode::Leaf { tab_id, fields, .. } => fields
            .validate()
            .map_err(|error| format!("tab={tab_id}: {error}")),
        codeterm_storage::DeskLayoutNode::Split { children, .. } => {
            for child in children {
                validate_layout(child)?;
            }
            Ok(())
        }
    }
}

fn capture_snapshot(
    snapshot: &SessionSnapshot,
    mut capture: impl FnMut(
        &SplitNodeSnapshot,
    ) -> Result<Vec<codeterm_types::agent::chat_types::ChatMsg>, String>,
) -> Result<(Value, Value, Vec<Value>), String> {
    let mut measurements = Vec::new();
    let mut histories = Vec::new();
    let mut all = Vec::new();
    for workspace in &snapshot.workspaces {
        for tab in &workspace.tabs {
            leaves(&tab.split_tree, &mut all);
        }
    }
    for node in all {
        let SplitNodeSnapshot::Leaf {
            tab_id,
            agent_type,
            chatbackend_plugin_id,
            surface,
            ..
        } = node
        else {
            continue;
        };
        let leaf = serde_json::to_value(node).map_err(|e| e.to_string())?;
        let prompt = leaf
            .pointer("/agent_identity/agent_runtime_config/systemPrompt")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let identity = leaf
            .pointer("/agent_identity/restorable_launch/identity_instructions")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let backing = if chatbackend_plugin_id.is_some() {
            "chat_backend"
        } else if surface.is_some() {
            "surface"
        } else {
            "pty"
        };
        let provider = chatbackend_plugin_id.as_deref().or(agent_type.as_deref());
        let (messages, status, reason) =
            if node.agent_session().is_some() || chatbackend_plugin_id.is_some() {
                match capture(node) {
                    Ok(messages) => (messages, "captured", None),
                    Err(error) => (Vec::new(), "unavailable", Some(sanitize::filler(&error))),
                }
            } else if agent_type.is_some() {
                (
                    Vec::new(),
                    "unavailable",
                    Some("Session identity unavailable".into()),
                )
            } else {
                (Vec::new(), "not_applicable", None)
            };
        let history = serde_json::to_value(&messages).map_err(|e| e.to_string())?;
        let turns = messages
            .iter()
            .map(|message| message.tool_flow_id.as_deref().unwrap_or("fixture-turn"))
            .collect::<std::collections::HashSet<_>>()
            .len();
        let images = messages
            .iter()
            .flat_map(|message| message.attachments.iter().flatten());
        let image_items = images.clone().count()
            + messages
                .iter()
                .filter(|message| message.image_data.is_some())
                .count();
        let image_bytes = images
            .map(|image| image.data.as_ref().map_or(0, String::len))
            .sum::<usize>()
            + messages
                .iter()
                .filter_map(|message| message.image_data.as_ref())
                .map(String::len)
                .sum::<usize>();
        measurements.push(json!({"tab_id":tab_id,"leaf_bytes":serde_json::to_vec(&leaf).map_err(|e| e.to_string())?.len(),
            "prompt_bytes":prompt.len(),"identity_bytes":identity.len(),"history_bytes":serde_json::to_vec(&history).map_err(|e| e.to_string())?.len(),
            "history_pages":messages.len().div_ceil(100),"history_items":messages.len(),"normalized_turn_groups":turns,
            "image_items":image_items,"image_bytes":image_bytes}));
        histories.push(json!({"tab_id":tab_id,"page_size":100,"messages":history,
            "transcript":{"status":status,"backing_kind":backing,"provider":provider,"reason":reason}}));
    }
    let safe_snapshot =
        sanitize::sanitize(&serde_json::to_value(&snapshot).map_err(|e| e.to_string())?)?;
    let safe_histories = sanitize::sanitize_plans(&json!(histories))?;
    Ok((safe_snapshot, safe_histories, measurements))
}

/// The one full plan read: each plan streams from SQL in turn and never enters the plan cache.
fn exported_plans(
    store: &codeterm_service_core::service::plan::PlanStore,
) -> Result<Value, String> {
    let mut plans = Vec::new();
    let mut failure = None;
    store.for_each_plan_uncached(|plan| match serde_json::to_value(&plan) {
        Ok(value) => plans.push(value),
        Err(error) => failure = failure.take().or(Some(error.to_string())),
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(Value::Array(plans)),
    }
}

pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, Failure> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let value = tokio::task::spawn_blocking(move || {
        let snapshot = if name == "latest" {
            svc.mux.read().to_session_snapshot()
        } else {
            svc.export_session_backup(&name)?
        };
        let (safe_snapshot, safe_histories, measurements) = capture_snapshot(&snapshot, |node| {
            let SplitNodeSnapshot::Leaf { cwd, .. } = node else {
                return Err("Transcript requires a leaf".into());
            };
            let (provider, session) = node.agent_session().ok_or("Session identity unavailable")?;
            codeterm_service_core::service::tab_chat::session_chat_snapshot(
                &svc,
                provider,
                cwd.as_deref().unwrap_or_default(),
                session,
                0,
            )
            .map(|(messages, _)| messages)
        })?;
        let plans = sanitize::sanitize_plans(&exported_plans(&svc.plan_store)?)?;
        serde_json::from_value::<Vec<codeterm_types::plan::Plan>>(plans.clone())
            .map_err(|error| format!("Sanitized plan schema refused: {error}"))?;
        let live = svc.mux.read().snapshot();
        let mut live_tab_metadata = Vec::new();
        for tab in live.tabs.values() {
            let mut metadata =
                json!({"tab_id":tab.id,"hidden":tab.hidden,"show_in_tabbar":tab.show_in_tabbar});
            if tab.hidden {
                metadata["tab"] = sanitize::sanitize_plans(
                    &serde_json::to_value(tab).map_err(|e| e.to_string())?,
                )?;
            }
            live_tab_metadata.push(metadata);
        }
        Ok::<_, String>(json!({"format":"upgrade-restore-v1","sanitized":true,
            "snapshot":safe_snapshot,"histories":safe_histories,"measurements":measurements,
            "plans":plans,"live_tab_metadata":live_tab_metadata}))
    })
    .await
    .map_err(failure)?
    .map_err(failure)?;
    Ok(Json(value))
}

pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(document): Json<Value>,
) -> Result<Json<Value>, Failure> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if !import_target_allowed(
        codeterm_paths::is_dev_instance(),
        state.api_port,
        &codeterm_paths::instance_suffix(),
    ) {
        return Err((
            StatusCode::FORBIDDEN,
            "Fixture import requires a named upgrade-restore-* DEV instance".into(),
        ));
    }
    if document["sanitized"] != true || document["format"] != "upgrade-restore-v1" {
        return Err(failure(
            "Import requires an upgrade-restore-v1 sanitized document",
        ));
    }
    let mut snapshot: SessionSnapshot =
        serde_json::from_value(document["snapshot"].clone()).map_err(failure)?;
    let mut plans: Vec<codeterm_types::plan::Plan> =
        serde_json::from_value(document["plans"].clone()).map_err(failure)?;
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        let fixture_cwd = codeterm_paths::data_dir().join("upgrade-restore-project");
        std::fs::create_dir_all(&fixture_cwd).map_err(|e| e.to_string())?;
        let directory = fixture_cwd.to_string_lossy().into_owned();
        for workspace in &mut snapshot.workspaces {
            workspace.cwd = Some(directory.clone());
            workspace.provisioned_cwd = None;
            for tab in &mut workspace.tabs {
                replay_cwd(&mut tab.split_tree, &directory);
            }
        }
        let candidate =
            codeterm_service_core::multiplexer::state::MultiplexerState::from_session_snapshot(
                &snapshot,
            );
        let count = candidate.snapshot().tabs.len();
        {
            let mux = svc.mux.read();
            if mux.snapshot().tabs.values().any(|tab| tab.pty_id.is_some()) {
                return Err("Fixture import refuses a DEV desk owning PTYs".to_string());
            }
        }
        let authority = svc
            .try_ext::<codeterm_service_core::startup::StorageAuthority>()
            .ok_or("Storage authority unavailable")?;
        let storage = authority.storage().ok_or("Storage unavailable")?.clone();
        let projected =
            codeterm_service_core::session::store::project_snapshot(&snapshot, Vec::new())?;
        for workspace in &projected.workspaces {
            for tab in &workspace.tabs {
                validate_layout(&tab.layout)?;
            }
        }
        codeterm_service_core::session::store::StorageDeskAdapter::from_storage(storage)
            .import_snapshot(&snapshot, Vec::new())?;
        for plan in &mut plans {
            plan.project_path = directory.clone();
            plan.canonical_project_path = plan.project_path.clone();
            for task in &mut plan.tasks {
                if task.lane.worktree_path.is_some() {
                    task.lane.worktree_path = Some(directory.clone());
                }
                for attempt in &mut task.execution_attempts {
                    if attempt.worktree_path.is_some() {
                        attempt.worktree_path = Some(directory.clone());
                    }
                }
            }
            let updated_at = plan.updated_at;
            svc.plan_store
                .save(plan, updated_at)
                .map_err(|e| format!("plan={}: {e}", plan.id))?;
        }
        *svc.mux.write() = candidate;
        codeterm_service_core::service::agent_runtime::restore_all(&svc);
        svc.emit_state_changed(MuxEmitCause::SessionRestore);
        Ok::<_, String>(
            json!({"imported_tabs":count,"restart_required":true,"boot_id":svc.events.boot_id()}),
        )
    })
    .await
    .map_err(failure)?
    .map_err(failure)?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests;
