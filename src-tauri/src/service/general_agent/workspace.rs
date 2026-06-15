//! General workspace + pane setup in [`MultiplexerState`].

use super::{general_dir, GENERAL_ALIAS, GENERAL_PANE_TITLE, GENERAL_WORKSPACE_NAME, MAIN_ALIAS};

/// Ensure General workspace + pane exist in MultiplexerState.
/// Called BEFORE mux is wrapped in Arc<Mutex<>> — no locking needed.
/// This avoids the deadlock that occurs when creating workspace from a background thread.
pub fn ensure_general_workspace(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    default_chat_view: bool,
) {
    let general_cwd = general_dir().to_string_lossy().to_string();

    // Check by protected flag — the only reliable marker
    let protected_ws: Vec<_> = mux
        .list_workspaces()
        .iter()
        .filter(|ws| ws.protected)
        .cloned()
        .collect();

    // Clean up duplicates: keep first protected, close rest
    if protected_ws.len() > 1 {
        for ws in &protected_ws[1..] {
            mux.set_workspace_protected(&ws.id, false);
            let _ = mux.close_workspace(&ws.id);
            log::info!(
                "[general-agent] Cleaned up duplicate protected workspace {}",
                ws.id
            );
        }
    }

    if !protected_ws.is_empty() {
        let ws = &protected_ws[0];
        if ws.name != GENERAL_WORKSPACE_NAME {
            let _ = mux.rename_workspace(&ws.id, GENERAL_WORKSPACE_NAME);
        }
        mux.move_workspace_to_front(&ws.id);
        if let Some(pid) = ws.pane_ids.first() {
            let pid = pid.clone();
            let _ = mux.rename_pane(&pid, GENERAL_PANE_TITLE);
            // The sidebar reads tab.title, which create_tab seeded with a
            // random scientist name — pin the GA tab to "Main" too so the
            // user's anchor pane never shows up as "Edison"/"Yang"/etc.
            if let Some(tab_id) = mux.find_tab_for_pane(&pid) {
                let ws_id = ws.id.clone();
                let _ = mux.rename_tab(&ws_id, &tab_id, GENERAL_PANE_TITLE);
            }
            if let Some(pane) = mux.get_pane_mut(&pid) {
                pane.protected = true;
                if default_chat_view && !pane.chat_view {
                    pane.chat_view = true;
                }
            }
            mux.register_alias(GENERAL_ALIAS, &pid);
            mux.register_alias(MAIN_ALIAS, &pid);
            log::info!(
                "[general-agent] Existing General workspace, alias for pane {}",
                pid
            );
        }
        return;
    }

    // Create General workspace
    let ws_info = mux.create_workspace(
        GENERAL_WORKSPACE_NAME.to_string(),
        Some(general_cwd.clone()),
    );
    mux.set_workspace_protected(&ws_info.id, true);
    mux.move_workspace_to_front(&ws_info.id);

    match mux.create_tab(&ws_info.id) {
        Ok((tab_id, pane_id)) => {
            let _ = mux.rename_pane(&pane_id, GENERAL_PANE_TITLE);
            // Pin the tab label too — create_tab seeded it with a random name.
            let _ = mux.rename_tab(&ws_info.id, &tab_id, GENERAL_PANE_TITLE);
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.protected = true;
                pane.role = Some(crate::multiplexer::types::AgentRole::Orchestrator);
                pane.can_spawn = true;
                pane.cwd = Some(general_cwd);
                pane.chat_view = default_chat_view;
            }
            mux.register_alias(GENERAL_ALIAS, &pane_id);
            mux.register_alias(MAIN_ALIAS, &pane_id);
            // Make General the active workspace + focused pane on first create
            let _ = mux.set_active_workspace(&ws_info.id);
            let _ = mux.set_focused_pane(&pane_id);
            log::info!(
                "[general-agent] Created General workspace {} with pane {}",
                ws_info.id,
                pane_id
            );
        }
        Err(e) => {
            log::error!("[general-agent] Failed to create General tab: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::general_agent::{GENERAL_PANE_TITLE, GENERAL_WORKSPACE_NAME};

    fn general_pane_chat_view(mux: &MultiplexerState) -> bool {
        let ws = mux
            .list_workspaces()
            .iter()
            .find(|ws| ws.name == GENERAL_WORKSPACE_NAME)
            .expect("general workspace")
            .clone();
        let pane_id = ws.pane_ids.first().expect("general pane");
        mux.get_pane(pane_id).expect("pane").chat_view
    }

    #[test]
    fn general_workspace_created_with_default_chat_enabled_sets_chat_view() {
        let mut mux = MultiplexerState::empty();

        super::ensure_general_workspace(&mut mux, true);

        assert!(general_pane_chat_view(&mux));
    }

    #[test]
    fn general_workspace_created_with_default_chat_disabled_stays_terminal() {
        let mut mux = MultiplexerState::empty();

        super::ensure_general_workspace(&mut mux, false);

        assert!(!general_pane_chat_view(&mux));
    }

    #[test]
    fn general_workspace_repair_applies_default_to_existing_main_pane() {
        let (mut mux, _) = MultiplexerState::new();
        let ws_id = mux.list_workspaces()[0].id.clone();
        mux.set_workspace_protected(&ws_id, true);

        super::ensure_general_workspace(&mut mux, true);

        let ws = mux
            .list_workspaces()
            .iter()
            .find(|ws| ws.name == GENERAL_WORKSPACE_NAME)
            .expect("general workspace")
            .clone();
        let pane_id = ws.pane_ids.first().expect("general pane");
        let pane = mux.get_pane(pane_id).expect("pane");
        assert_eq!(pane.title, GENERAL_PANE_TITLE);
        assert!(pane.chat_view);
    }
}
