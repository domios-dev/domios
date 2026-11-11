use crate::routes::AppState;
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

const FORMAT: &str = "upgrade-restore-v2";
const PAGE_SIZE: usize = 100;

struct ByteCount(usize);

impl std::io::Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct HistoryShape {
    rows: Vec<Value>,
    groups: usize,
    image_items: usize,
    image_bytes: usize,
}

/// Replay synthesizes text from these sizes, so the export grows with message count, not history bytes.
fn history_shape(messages: &[codeterm_types::agent::chat_types::ChatMsg]) -> HistoryShape {
    let mut groups = std::collections::HashMap::new();
    let mut image_items = 0;
    let mut image_bytes = 0;
    let rows = messages
        .iter()
        .map(|message| {
            let next = groups.len();
            let group = *groups
                .entry(message.tool_flow_id.as_deref())
                .or_insert(next);
            let mut row = json!({"kind":message.kind,"group":group,"bytes":message.content.len()});
            if let Some(result) = &message.tool_result {
                row["result_bytes"] = json!(result.len());
            }
            if let Some(input) = &message.tool_input {
                row["input_bytes"] = json!(input.to_string().len());
            }
            let images: Vec<usize> = message
                .attachments
                .iter()
                .flatten()
                .map(|image| image.data.as_ref().map_or(0, String::len))
                .collect();
            image_items += images.len();
            image_bytes += images.iter().sum::<usize>();
            if !images.is_empty() {
                row["image_bytes"] = json!(images);
            }
            if let Some(data) = &message.image_data {
                image_items += 1;
                image_bytes += data.len();
                row["image_data_bytes"] = json!(data.len());
            }
            row
        })
        .collect();
    HistoryShape {
        rows,
        groups: groups.len(),
        image_items,
        image_bytes,
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
        let shape = history_shape(&messages);
        let mut history_bytes = ByteCount(0);
        serde_json::to_writer(&mut history_bytes, &messages).map_err(|e| e.to_string())?;
        measurements.push(json!({"tab_id":tab_id,"leaf_bytes":serde_json::to_vec(&leaf).map_err(|e| e.to_string())?.len(),
            "prompt_bytes":prompt.len(),"identity_bytes":identity.len(),"history_bytes":history_bytes.0,
            "history_pages":messages.len().div_ceil(PAGE_SIZE),"history_items":messages.len(),
            "normalized_turn_groups":shape.groups,"image_items":shape.image_items,"image_bytes":shape.image_bytes}));
        histories.push(json!({"tab_id":tab_id,"page_size":PAGE_SIZE,"shape":shape.rows,
            "transcript":{"status":status,"backing_kind":backing,"provider":provider,"reason":reason}}));
    }
    let safe_snapshot =
        sanitize::sanitize(&serde_json::to_value(&snapshot).map_err(|e| e.to_string())?)?;
    let safe_histories = sanitize::sanitize_plans(&json!(histories))?;
    Ok((safe_snapshot, safe_histories, measurements))
}

/// Complete export encodes validated SQL records without constructing a Plan.
fn exported_plans(
    store: &codeterm_service_core::service::plan::PlanStore,
) -> Result<Value, String> {
    let plans = store
        .export_all_plan_documents_with(&sanitize::SanitizedPlanRows)
        .map_err(|error| error.to_string())?;
    Ok(Value::Array(plans))
}

pub async fn export(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, Failure> {
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
        let plans = exported_plans(&svc.plan_store)?;
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
        Ok::<_, String>(json!({"format":FORMAT,"sanitized":true,
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
    _headers: HeaderMap,
    Json(document): Json<Value>,
) -> Result<Json<Value>, Failure> {
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
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        let (mut snapshot, mut plans) = fixture_document(&document)?;
        let fixture_cwd = codeterm_paths::data_dir().join("upgrade-restore-project");
        let directory = fixture_cwd.to_string_lossy().into_owned();
        for workspace in &mut snapshot.workspaces {
            workspace.cwd = Some(directory.clone());
            workspace.provisioned_cwd = None;
            for tab in &mut workspace.tabs {
                replay_cwd(&mut tab.split_tree, &directory);
            }
        }
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
        }
        let count = codeterm_service_core::service::session_persist::import_fixture(
            &svc,
            snapshot,
            plans,
            move || std::fs::create_dir_all(&fixture_cwd).map_err(|error| error.to_string()),
        )?;
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

fn fixture_document(
    document: &Value,
) -> Result<(SessionSnapshot, Vec<codeterm_types::plan::Plan>), String> {
    if document["sanitized"] != true || document["format"] != FORMAT {
        return Err(format!("Import requires an {FORMAT} sanitized document"));
    }
    let snapshot: SessionSnapshot =
        serde_json::from_value(document["snapshot"].clone()).map_err(|error| error.to_string())?;
    let plans =
        serde_json::from_value(document["plans"].clone()).map_err(|error| error.to_string())?;
    codeterm_service_core::service::session_persist::validate_fixture_snapshot(&snapshot)?;
    Ok((snapshot, plans))
}

#[cfg(test)]
mod tests;
