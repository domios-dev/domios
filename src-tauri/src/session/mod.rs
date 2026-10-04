pub mod lifecycle {
    pub use codeterm_pty::session::lifecycle::*;
}
pub mod scrollback {
    pub use codeterm_pty::session::scrollback::*;
}
pub(crate) mod scrollback_restore;
pub(crate) mod scrollback_sync;
pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;

use crate::multiplexer::commands::MuxState;
pub use store::{SessionSnapshot, SessionStore};

pub type SessionState = Arc<Mutex<SessionStore>>;

pub(crate) fn save_current_snapshot(
    session: &SessionState,
    mux: &MuxState,
    intent: store::SaveIntent,
) -> Result<store::SaveOutcome, String> {
    save_current_snapshot_after_lock(session, mux, intent, || {})
}

fn save_current_snapshot_after_lock(
    session: &SessionState,
    mux: &MuxState,
    intent: store::SaveIntent,
    after_lock: impl FnOnce(),
) -> Result<store::SaveOutcome, String> {
    let store = session.lock();
    after_lock();
    let snapshot = mux.read().to_session_snapshot();
    store
        .save_with_intent(&snapshot, intent)
        .map_err(|error| format!("save session: {error}"))
}

pub(crate) fn save_current_snapshot_named(
    session: &SessionState,
    mux: &MuxState,
    name: &str,
) -> Result<(), String> {
    let store = session.lock();
    let snapshot = mux.read().to_session_snapshot();
    store
        .save_named(name, &snapshot)
        .map_err(|error| format!("save session: {error}"))
}

pub(crate) fn save_captured_snapshot_with_live_identities(
    session: &SessionState,
    mux: &MuxState,
    captured: &SessionSnapshot,
    intent: store::SaveIntent,
) -> Result<store::SaveOutcome, String> {
    let store = session.lock();
    let live = mux.read().to_session_snapshot();
    let mut snapshot = captured.clone();
    merge_live_agent_identities(&mut snapshot, &live);
    store
        .save_with_intent(&snapshot, intent)
        .map_err(|error| format!("save session: {error}"))
}

fn merge_live_agent_identities(snapshot: &mut SessionSnapshot, live: &SessionSnapshot) {
    use std::collections::HashMap;
    use store::{AgentIdentitySnapshot, SplitNodeSnapshot};

    fn collect(node: &SplitNodeSnapshot, identities: &mut HashMap<String, AgentIdentitySnapshot>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_identity: Some(identity),
                ..
            } => {
                identities.insert(tab_id.clone(), identity.clone());
            }
            SplitNodeSnapshot::Leaf { .. } => {}
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect(child, identities);
                }
            }
        }
    }

    fn apply(node: &mut SplitNodeSnapshot, identities: &HashMap<String, AgentIdentitySnapshot>) {
        match node {
            SplitNodeSnapshot::Leaf {
                tab_id,
                agent_identity,
                ..
            } => {
                if let Some(identity) = identities.get(tab_id) {
                    *agent_identity = Some(identity.clone());
                }
            }
            SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    apply(child, identities);
                }
            }
        }
    }

    let mut identities = HashMap::new();
    for workspace in &live.workspaces {
        for tab in &workspace.tabs {
            collect(&tab.split_tree, &mut identities);
        }
    }
    for workspace in &mut snapshot.workspaces {
        for tab in &mut workspace.tabs {
            apply(&mut tab.split_tree, &identities);
        }
    }
}

#[tauri::command]
pub async fn save_session(
    app: tauri::AppHandle,
    mux: tauri::State<'_, MuxState>,
    _session: tauri::State<'_, SessionState>,
) -> Result<(), String> {
    let app = app.clone();
    let mux = mux.inner().clone();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::service::session_persist::write_session_snapshot(&app, &mux)
    })
    .await
    .map_err(|error| format!("session persistence task failed: {error}"))??;
    if let store::SaveOutcome::RefusedReduction(refusal) = outcome {
        log::error!("[session] explicit save preserved the fuller desk: {refusal}");
    }
    Ok(())
}

#[tauri::command]
pub async fn save_session_named(
    mux: tauri::State<'_, MuxState>,
    session: tauri::State<'_, SessionState>,
    name: String,
) -> Result<(), String> {
    let mux = mux.inner().clone();
    let session = session.inner().clone();
    tokio::task::spawn_blocking(move || save_current_snapshot_named(&session, &mux, &name))
        .await
        .map_err(|error| format!("named session save task failed: {error}"))?
}

#[tauri::command]
pub fn list_saved_sessions(session: tauri::State<'_, SessionState>) -> Result<Vec<String>, String> {
    session.lock().list_saved()
}

#[tauri::command]
pub fn load_session(
    session: tauri::State<'_, SessionState>,
    name: Option<String>,
) -> Result<Option<SessionSnapshot>, String> {
    let store = session.lock();
    match name {
        Some(n) => store.load_named(&n),
        None => store.load_last(),
    }
}

#[cfg(test)]
mod tests;
