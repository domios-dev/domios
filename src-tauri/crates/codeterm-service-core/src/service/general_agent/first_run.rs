//! First-run General Agent provider picker — tab marker + completion flow.

use crate::multiplexer::types::TabType;
use crate::service::MuxEmitCause;

use super::workspace::{default_chat_view_for_provider, ensure_general_workspace_without_persist};
use super::{generate_instructions, start_general_agent, GENERAL_ALIAS};

pub const FIRST_RUN_TAB_MARKER: &str = "codeterm:first-run-general-agent";
const FIRST_RUN_TAB_TITLE: &str = "Get started";

pub fn needs_first_run_picker(enabled: bool, first_run_complete: bool) -> bool {
    enabled && !first_run_complete
}

pub fn snapshot_has_protected_main(mux: &crate::multiplexer::state::MultiplexerState) -> bool {
    mux.list_workspaces().iter().any(|ws| {
        ws.protected
            && ws
                .tab_ids
                .iter()
                .any(|pid| mux.get_tab(pid).is_some_and(|p| p.protected))
    })
}

fn has_first_run_tab(mux: &crate::multiplexer::state::MultiplexerState) -> bool {
    mux.list_workspaces().iter().any(|ws| {
        ws.tab_ids.iter().any(|pid| {
            mux.get_tab(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_TAB_MARKER)
        })
    })
}

/// Create the in-tab first-run picker when GA is enabled but not yet configured.
pub fn ensure_first_run_tab(mux: &mut crate::multiplexer::state::MultiplexerState) {
    if mux.resolve_tab_id(GENERAL_ALIAS).is_some() || has_first_run_tab(mux) {
        return;
    }

    // Protect the picker workspace only when it is the instance's sole content, because closing it otherwise leaves CodeTerm empty.
    let ws_id = if mux.list_workspaces().is_empty() {
        let id = mux.create_workspace("Welcome".to_string(), None).id;
        mux.set_workspace_protected(&id, true);
        id
    } else {
        mux.list_workspaces()[0].id.clone()
    };
    let ws_protected = mux.get_workspace(&ws_id).is_some_and(|w| w.protected);

    match mux.create_tab(&ws_id) {
        Ok(tab_id) => {
            let _ = mux.rename_tab(&tab_id, FIRST_RUN_TAB_TITLE);
            let _ = mux.set_tab_description(&tab_id, Some(FIRST_RUN_TAB_MARKER.to_string()));
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.tab_type = TabType::Choice;
                // Picker is ephemeral — must not survive a daemon restart as a ghost tab.
                tab.skip_snapshot = true;
                // In a protected (sole-content) workspace the picker can't be closed
                // into an empty CodeTerm; the completion flow unprotects it first.
                tab.protected = ws_protected;
            }
            let _ = mux.set_active_workspace(&ws_id);
            let _ = mux.set_focused_tab(&tab_id);
            log::info!("[general-agent] Opened first-run picker tab {}", tab_id);
        }
        Err(e) => log::error!("[general-agent] Failed to open first-run tab: {}", e),
    }
}

/// Called once at daemon startup after `attach_closed_store`. Scans all
/// restored workspaces for ephemeral tabs (e.g. first-run picker tabs that
/// leaked into the session snapshot before `skip_snapshot` was set), drops
/// them via `close_tab` so they appear in Recently Closed, and emits an INFO
/// log per drop. Returns the number of tabs purged.
pub fn purge_snapshot_ghosts(mux: &mut crate::multiplexer::state::MultiplexerState) -> usize {
    purge_snapshot_ghosts_with_evictions(mux).0
}

/// Purge ephemeral restored tabs and report any older closed tabs evicted by
/// closed-history retention so their per-tab records can be cleaned up.
pub fn purge_snapshot_ghosts_with_evictions(
    mux: &mut crate::multiplexer::state::MultiplexerState,
) -> (usize, Vec<String>) {
    // Collect (ws_id, tab_id) while holding only an immutable borrow.
    let mut to_drop: Vec<(String, String)> = Vec::new();
    for ws_info in mux.list_workspaces() {
        if let Some(ws) = mux.get_workspace(&ws_info.id) {
            for tab_id in &ws.tabs {
                if mux.get_tab(tab_id).and_then(|p| p.description.as_deref())
                    == Some(FIRST_RUN_TAB_MARKER)
                {
                    to_drop.push((ws_info.id.clone(), tab_id.clone()));
                }
            }
        }
    }
    let count = to_drop.len();
    for (ws_id, tab_id) in to_drop {
        log::info!(
            "[restore] dropping ghost tab {} in workspace {} — ephemeral picker Tab leaked into session snapshot; moved to Recently Closed",
            tab_id, ws_id
        );
        // Sole-content pickers are close-protected; a genuine ghost purge must
        // still drop them, so lift the guard first.
        if mux
            .get_workspace(&ws_id)
            .is_some_and(|ws| ws.tabs.contains(&tab_id))
        {
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.protected = false;
            }
        }
        let _ = mux.close_tab(&tab_id);
    }
    let evicted = mux
        .take_closed_history_batch()
        .map(|batch| batch.persist_on_blocking_thread_collect_evicted())
        .unwrap_or_default();
    (count, evicted)
}

fn close_first_run_tabs(
    mux: &mut crate::multiplexer::state::MultiplexerState,
) -> Result<(), String> {
    let tab_ids: Vec<String> = mux
        .list_workspaces()
        .iter()
        .flat_map(|ws| ws.tab_ids.iter().cloned())
        .filter(|pid| {
            mux.get_tab(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_TAB_MARKER)
        })
        .collect();
    for tab_id in tab_ids {
        // Sole-content picker tabs are protected against an empty-CodeTerm close;
        // completion legitimately retires them, so lift the guard first.
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            tab.protected = false;
        }
        let _ = mux.close_tab(&tab_id);
    }
    Ok(())
}

fn persist_first_run_completion(
    snapshot: &crate::config::schema::AppConfig,
    ui: &dyn crate::ports::host::UiEvents,
    save: impl FnOnce(&crate::config::schema::AppConfig) -> Result<(), String>,
) -> Result<(), String> {
    save(snapshot)?;
    ui.emit(
        "config-changed",
        serde_json::to_value(snapshot).map_err(|error| error.to_string())?,
    );
    Ok(())
}

fn prepare_first_run_general(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    default_chat_view: bool,
) -> Result<usize, String> {
    close_first_run_tabs(mux)?;
    let closed = ensure_general_workspace_without_persist(mux, default_chat_view);
    let tab = mux
        .resolve_tab_id(GENERAL_ALIAS)
        .ok_or("General tab is unavailable")?;
    let workspace = mux
        .find_workspace_for_tab(&tab)
        .ok_or("General workspace is unavailable")?;
    mux.set_active_workspace(&workspace)?;
    mux.set_active_tab(&workspace, &tab)?;
    mux.set_focused_tab(&tab)?;
    Ok(closed)
}

pub fn complete_first_run_core(
    svc: &crate::ServiceState,
    config: &crate::config::ConfigState,
    provider_id: &str,
    full_access: bool,
) -> Result<(), String> {
    let known = svc
        .agent_registry
        .with_provider(provider_id, |_| ())
        .is_some();
    if !known {
        return Err(format!("Unknown provider: {}", provider_id));
    }
    let (default_chat_view, mem, mux, pty, screen_state) = {
        let mut snapshot = config.lock().clone();
        snapshot.general_agent.agent_provider = provider_id.to_string();
        snapshot.general_agent.first_run_complete = true;
        snapshot.agent.full_access = Some(full_access);
        let default_chat_view =
            default_chat_view_for_provider(&snapshot, &svc.agent_registry, provider_id);
        persist_first_run_completion(&snapshot, svc.ui.as_ref(), |snapshot| {
            crate::config::save_config_and_apply(config, snapshot.clone())
        })?;
        (
            default_chat_view,
            svc.mem.clone(),
            svc.mux.clone(),
            svc.pty.clone(),
            svc.screen_state.clone(),
        )
    };

    super::state::clear_session_id();
    super::state::require_fresh_session()?;

    let (close_result, history_batch) = {
        let mut mux_guard = mux.write();
        let result = prepare_first_run_general(&mut mux_guard, default_chat_view);
        (result, mux_guard.take_closed_history_batch())
    };
    if let Some(batch) = history_batch {
        let evicted = batch.persist_on_blocking_thread_collect_evicted();
        svc.forget_evicted_tab_todos(evicted);
    }
    let duplicate_generals_closed = close_result?;
    if duplicate_generals_closed > 0 {
        // Duplicate-General cleanup is a deliberate reduction of persisted
        // workspaces; persist it with intent so the guard has nothing to refuse.
        crate::service::session_persist::persist_session_after_close(svc.as_ref());
    }
    svc.emit_state_changed(MuxEmitCause::GeneralAgent);

    let provider_owned = provider_id.to_string();
    let config = config.clone();
    let svc = svc.clone();
    std::thread::spawn(move || {
        if let Err(error) = super::instructions::ensure_bootstrap_instructions() {
            log::error!("[general-agent] Failed to write bootstrap instructions: {error}");
        }
        let capability_hints = config.lock().general_agent.capability_hints;
        generate_instructions(capability_hints, &provider_owned, Some(&mem));
        start_general_agent(&svc, &mux, &pty, &screen_state, &config);
    });
    Ok(())
}

#[cfg(test)]
mod tests;
