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

    // A brand-new instance has no workspaces: the picker is the ONLY content, so
    // the workspace hosting it must be protected — otherwise closing that sole
    // tab/workspace leaves an empty CodeTerm (P1). When other workspaces already
    // exist, closing the picker still leaves them, so no protection is needed.
    let ws_id = if mux.list_workspaces().is_empty() {
        let id = mux.create_workspace("Welcome".to_string(), None).id;
        mux.set_workspace_protected(&id, true);
        id
    } else {
        mux.list_workspaces()[0].id.clone()
    };
    let ws_protected = mux.get_workspace(&ws_id).is_some_and(|w| w.protected);

    match mux.create_tab(&ws_id) {
        Ok((tab_id, pane_id)) => {
            let _ = mux.rename_tab(&ws_id, &tab_id, FIRST_RUN_TAB_TITLE);
            let _ = mux.rename_pane(&pane_id, FIRST_RUN_TAB_TITLE);
            let _ = mux.set_pane_description(&pane_id, Some(FIRST_RUN_PANE_MARKER.to_string()));
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                // Picker is ephemeral — must not survive a daemon restart as a ghost tab.
                pane.skip_snapshot = true;
                // In a protected (sole-content) workspace the picker can't be closed
                // into an empty CodeTerm; the completion flow unprotects it first.
                pane.protected = ws_protected;
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
        // Sole-content pickers are close-protected; a genuine ghost purge must
        // still drop them, so lift the guard first.
        if let Some(pane_id) = mux.get_workspace(&ws_id).and_then(|ws| {
            ws.tabs
                .iter()
                .find(|t| t.id == tab_id)
                .map(|t| t.pane_id.clone())
        }) {
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.protected = false;
            }
        }
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
        // Sole-content picker panes are protected against an empty-CodeTerm close;
        // completion legitimately retires them, so lift the guard first.
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.protected = false;
        }
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
    super::state::clear_session_id();
    super::state::require_fresh_session()?;

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
        if let Err(error) = super::instructions::ensure_bootstrap_instructions() {
            log::error!("[general-agent] Failed to write bootstrap instructions: {error}");
        }
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

    fn first_run_picker_pane_id(mux: &MultiplexerState) -> String {
        mux.list_workspaces()
            .iter()
            .flat_map(|ws| ws.pane_ids.iter().cloned())
            .find(|pid| {
                mux.get_pane(pid).and_then(|p| p.description.as_deref())
                    == Some(FIRST_RUN_PANE_MARKER)
            })
            .expect("first-run picker pane exists")
    }

    #[test]
    fn first_run_sole_picker_workspace_and_pane_are_protected_against_empty_close() {
        // P1: on a brand-new instance the picker is the ONLY content — closing it
        // must never leave an empty CodeTerm.
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);

        let ws = mux.list_workspaces()[0].clone();
        assert!(ws.protected, "sole first-run workspace must be protected");
        let pane_id = first_run_picker_pane_id(&mux);
        assert!(
            mux.get_pane(&pane_id).unwrap().protected,
            "sole first-run picker pane must be protected"
        );
        assert!(
            mux.close_workspace(&ws.id).is_err(),
            "closing the sole first-run workspace must be refused"
        );
        assert!(
            mux.close_pane(&pane_id).is_err(),
            "closing the sole first-run picker pane must be refused"
        );
        // Invariant holds: still a workspace with a valid pane.
        assert_eq!(mux.list_workspaces().len(), 1);
        assert!(!mux.list_workspaces()[0].pane_ids.is_empty());
    }

    #[test]
    fn first_run_picker_not_protected_when_other_workspaces_exist() {
        // Closing the picker here still leaves the pre-existing workspace, so no
        // protection is needed — and over-protecting a user workspace is wrong.
        let (mut mux, _) = MultiplexerState::new();
        ensure_first_run_tab(&mut mux);
        let pane_id = first_run_picker_pane_id(&mux);
        assert!(
            !mux.get_pane(&pane_id).unwrap().protected,
            "picker sharing a non-protected workspace must stay closable"
        );
    }

    #[test]
    fn close_first_run_tabs_retires_the_protected_picker() {
        // Completion legitimately closes the (protected) picker before General is
        // materialized — the guard must be lifted, not block the transition.
        let mut mux = MultiplexerState::empty();
        ensure_first_run_tab(&mut mux);
        assert!(has_first_run_tab(&mux));
        close_first_run_tabs(&mut mux).expect("retire first-run tabs");
        assert!(
            !has_first_run_tab(&mux),
            "picker pane removed on completion"
        );
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

    // ── Tests moved from multiplexer::state::snapshot::tests ─────────────

    #[test]
    fn ghost_repro_restore_from_old_snapshot_drops_picker_pane() {
        use crate::session::store::{
            SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
        };

        let old_snapshot = SessionSnapshot {
            version: "test".into(),
            timestamp: 0,
            active_workspace_id: Some("ws".into()),
            teams: vec![],
            workspaces: vec![WorkspaceSnapshot {
                id: "ws".into(),
                name: "Welcome".into(),
                active_tab_id: Some("tab-picker".into()),
                cwd: None,
                protected: false,
                sort_order: 0,
                tabs: vec![TabSnapshot {
                    id: "tab-picker".into(),
                    title: "Get started".into(),
                    description: Some(FIRST_RUN_PANE_MARKER.into()),
                    split_tree: SplitNodeSnapshot::Leaf {
                        pane_id: "pane-picker".into(),
                        cwd: None,
                        title: Some("Get started".into()),
                        description: Some(FIRST_RUN_PANE_MARKER.into()),
                        title_level: Some("User".into()),
                        agent_type: None,
                        agent_type_locked: false,
                        pending_delivery: None,
                        claude_session_id: None,
                        agent_identity: None,
                        role: None,
                        role_profile: None,
                        team_id: None,
                        parent_pane_id: None,
                        spawned_by_pane_id: None,
                        report_target_pane_id: None,
                        report_target_mesh_peer: None,
                        can_spawn: false,
                        depth: 0,
                        chat_view: Some(false),
                        auto_approve: false,
                        agent_session_model: None,
                        agent_planned_model: None,
                        agent_requested_model: None,
                        agent_provider_resolved_model: None,
                        model_fallback: None,
                        model_identity_mismatch: None,
                        chatbackend_plugin_id: None,
                        chatbackend_session_id: None,
                        watcher: None,
                    },
                    sort_order: 0,
                }],
            }],
        };

        let mut restored = MultiplexerState::from_session_snapshot(&old_snapshot);
        assert!(
            restored.get_pane("pane-picker").is_some(),
            "pre-purge: picker pane must be present in restored state"
        );
        assert_eq!(
            restored.get_workspace("ws").map(|w| w.tabs.len()),
            Some(1),
            "pre-purge: workspace must have the ghost tab"
        );

        let dropped = purge_snapshot_ghosts(&mut restored);
        assert_eq!(dropped, 1, "purge must drop exactly one ghost tab");

        assert!(
            restored.get_pane("pane-picker").is_none(),
            "post-purge: picker pane must be absent"
        );
        assert_eq!(
            restored.get_workspace("ws").map(|w| w.tabs.len()),
            Some(0),
            "post-purge: workspace must have no tabs"
        );
        let closed = restored.list_recently_closed("ws");
        assert!(
            !closed.is_empty(),
            "dropped ghost tab must appear in Recently Closed"
        );
    }

    #[test]
    fn ghost_repro_first_run_picker_excluded_from_snapshot() {
        let mut state = MultiplexerState::empty();
        ensure_first_run_tab(&mut state);

        assert!(!state.list_workspaces().is_empty());
        assert!(!state.list_workspaces()[0].pane_ids.is_empty());

        let snap = state.to_session_snapshot();
        assert_eq!(
            snap.workspaces[0].tabs.len(),
            0,
            "first-run picker pane must be excluded from the session snapshot"
        );
    }
}
