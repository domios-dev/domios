//! First-run General Agent provider picker — tab marker + completion flow.

use tauri::AppHandle;

use super::workspace::{default_chat_view_for_provider, ensure_general_workspace};
use super::{generate_instructions, start_general_agent, GENERAL_ALIAS};

pub const FIRST_RUN_PANE_MARKER: &str = "codeterm:first-run-general-agent";
const FIRST_RUN_TAB_TITLE: &str = "Get started";

pub fn needs_first_run_picker(enabled: bool, first_run_complete: bool) -> bool {
    enabled && !first_run_complete
}

pub fn snapshot_has_protected_main(mux: &crate::multiplexer::state::MultiplexerState) -> bool {
    mux.list_workspaces().iter().any(|ws| {
        ws.protected
            && ws
                .pane_ids
                .iter()
                .any(|pid| mux.get_pane(pid).is_some_and(|p| p.protected))
    })
}

fn has_first_run_tab(mux: &crate::multiplexer::state::MultiplexerState) -> bool {
    mux.list_workspaces().iter().any(|ws| {
        ws.pane_ids.iter().any(|pid| {
            mux.get_pane(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_PANE_MARKER)
        })
    })
}

/// Create the in-tab first-run picker when GA is enabled but not yet configured.
pub fn ensure_first_run_tab(mux: &mut crate::multiplexer::state::MultiplexerState) {
    if mux.resolve_pane_id(GENERAL_ALIAS).is_some() || has_first_run_tab(mux) {
        return;
    }

    let ws_id = if mux.list_workspaces().is_empty() {
        mux.create_workspace("Welcome".to_string(), None).id
    } else {
        mux.list_workspaces()[0].id.clone()
    };

    match mux.create_tab(&ws_id) {
        Ok((tab_id, pane_id)) => {
            let _ = mux.rename_tab(&ws_id, &tab_id, FIRST_RUN_TAB_TITLE);
            let _ = mux.rename_pane(&pane_id, FIRST_RUN_TAB_TITLE);
            let _ = mux.set_pane_description(&pane_id, Some(FIRST_RUN_PANE_MARKER.to_string()));
            // Picker is ephemeral — must not survive a daemon restart as a ghost tab.
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.skip_snapshot = true;
            }
            let _ = mux.set_active_workspace(&ws_id);
            let _ = mux.set_focused_pane(&pane_id);
            log::info!("[general-agent] Opened first-run picker tab {}", pane_id);
        }
        Err(e) => log::error!("[general-agent] Failed to open first-run tab: {}", e),
    }
}

/// Called once at daemon startup after `attach_closed_store`. Scans all
/// restored workspaces for ephemeral panes (e.g. first-run picker tabs that
/// leaked into the session snapshot before `skip_snapshot` was set), drops
/// them via `close_tab` so they appear in Recently Closed, and emits an INFO
/// log per drop. Returns the number of tabs purged.
pub fn purge_snapshot_ghosts(mux: &mut crate::multiplexer::state::MultiplexerState) -> usize {
    // Collect (ws_id, tab_id) while holding only an immutable borrow.
    let mut to_drop: Vec<(String, String)> = Vec::new();
    for ws_info in mux.list_workspaces() {
        if let Some(ws) = mux.get_workspace(&ws_info.id) {
            for tab in &ws.tabs {
                if mux
                    .get_pane(&tab.pane_id)
                    .and_then(|p| p.description.as_deref())
                    == Some(FIRST_RUN_PANE_MARKER)
                {
                    to_drop.push((ws_info.id.clone(), tab.id.clone()));
                }
            }
        }
    }
    let count = to_drop.len();
    for (ws_id, tab_id) in to_drop {
        log::info!(
            "[restore] dropping ghost tab {} in workspace {} — ephemeral picker pane leaked into session snapshot; moved to Recently Closed",
            tab_id, ws_id
        );
        let _ = mux.close_tab(&ws_id, &tab_id);
    }
    count
}

fn close_first_run_tabs(
    mux: &mut crate::multiplexer::state::MultiplexerState,
) -> Result<(), String> {
    let pane_ids: Vec<String> = mux
        .list_workspaces()
        .iter()
        .flat_map(|ws| ws.pane_ids.iter().cloned())
        .filter(|pid| {
            mux.get_pane(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_PANE_MARKER)
        })
        .collect();
    for pane_id in pane_ids {
        let _ = mux.close_pane(&pane_id);
    }
    Ok(())
}

pub fn complete_first_run_core(
    svc: &crate::ServiceState,
    config: &crate::config::ConfigState,
    app: &AppHandle,
    provider_id: &str,
) -> Result<(), String> {
    let known = svc
        .agent_registry
        .with_provider(provider_id, |_| ())
        .is_some();
    if !known {
        return Err(format!("Unknown provider: {}", provider_id));
    }

    let (default_chat_view, mem, mux, pty, screen_state) = {
        let mut cfg = config.lock();
        cfg.general_agent.agent_provider = provider_id.to_string();
        cfg.general_agent.first_run_complete = true;
        let snapshot = cfg.clone();
        let default_chat_view =
            default_chat_view_for_provider(&snapshot, &svc.agent_registry, provider_id);
        drop(cfg);
        crate::config::save_config_pub(&snapshot)?;
        (
            default_chat_view,
            svc.mem.clone(),
            svc.mux.clone(),
            svc.pty.clone(),
            svc.screen_state.clone(),
        )
    };

    {
        let mut mux_guard = mux.write();
        close_first_run_tabs(&mut mux_guard)?;
        ensure_general_workspace(&mut mux_guard, default_chat_view);
    }
    svc.emit_state_changed();

    let provider_owned = provider_id.to_string();
    let config = config.clone();
    let svc = svc.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        generate_instructions(&provider_owned, Some(&mem));
        start_general_agent(&svc, &mux, &pty, &screen_state, &config, &app);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    fn needs_picker_when_enabled_and_incomplete() {
        assert!(needs_first_run_picker(true, false));
        assert!(!needs_first_run_picker(true, true));
        assert!(!needs_first_run_picker(false, false));
    }

    #[test]
    fn ensure_first_run_tab_creates_marker_pane() {
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);
        assert!(has_first_run_tab(&mux));
        assert!(mux.resolve_pane_id(GENERAL_ALIAS).is_none());
    }

    #[test]
    fn ensure_first_run_tab_is_idempotent() {
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);
        let count = mux.list_workspaces()[0].pane_ids.len();
        ensure_first_run_tab(&mut mux);
        assert_eq!(mux.list_workspaces()[0].pane_ids.len(), count);
    }

    #[test]
    fn first_run_picker_pane_has_skip_snapshot_set() {
        // The picker must not survive a daemon restart as a ghost tab.
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);
        let pane_id = mux.list_workspaces()[0].pane_ids[0].clone();
        let pane = mux.get_pane(&pane_id).expect("picker pane exists");
        assert!(
            pane.skip_snapshot,
            "first-run picker pane must have skip_snapshot=true to be excluded from the session snapshot"
        );
    }

    #[test]
    fn purge_snapshot_ghosts_drops_picker_pane_and_records_to_closed() {
        // purge_snapshot_ghosts must drop it and record it in Recently Closed.
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);
        let ws_id = mux.list_workspaces()[0].id.clone();
        // Pane is present in live state (just not in the snapshot after the fix).
        assert!(!mux.get_workspace(&ws_id).unwrap().tabs.is_empty());

        let dropped = purge_snapshot_ghosts(&mut mux);
        assert_eq!(dropped, 1);

        // Tab removed from workspace.
        assert_eq!(mux.get_workspace(&ws_id).unwrap().tabs.len(), 0);
        // Pane removed from map.
        assert!(!mux.list_workspaces()[0].pane_ids.iter().any(|pid| {
            mux.get_pane(pid).and_then(|p| p.description.as_deref()) == Some(FIRST_RUN_PANE_MARKER)
        }));
        // Appears in Recently Closed.
        assert!(
            !mux.list_recently_closed(&ws_id).is_empty(),
            "purged ghost must appear in Recently Closed"
        );
    }

    #[test]
    fn purge_snapshot_ghosts_preserves_real_panes() {
        // A real pane (no ephemeral marker) must survive purge_snapshot_ghosts.
        let (mut mux, _) = MultiplexerState::new();
        let ws_id = mux.list_workspaces()[0].id.clone();
        let (_tab, pane_id) = mux.create_tab(&ws_id).unwrap();
        mux.get_pane_mut(&pane_id).unwrap().title = "claude".to_string();

        let dropped = purge_snapshot_ghosts(&mut mux);
        assert_eq!(dropped, 0);
        assert!(
            mux.get_pane(&pane_id).is_some(),
            "real pane must survive purge"
        );
    }
}
