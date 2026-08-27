//! General workspace + tab setup in [`MultiplexerState`].

use super::{general_dir, GENERAL_ALIAS, GENERAL_TAB_TITLE, GENERAL_WORKSPACE_NAME, MAIN_ALIAS};
use crate::agent::registry::AgentRegistry;
use crate::service::spawn::{should_default_agent_tab_to_chat, TabVisibility};

pub fn default_chat_view_for_provider(
    _config: &crate::config::schema::AppConfig,
    registry: &AgentRegistry,
    provider_id: &str,
) -> bool {
    let supports_chat_ui = registry
        .with_provider(provider_id, |provider| provider.supports_chat_ui())
        .unwrap_or(false);
    should_default_agent_tab_to_chat(TabVisibility::Visible, supports_chat_ui)
}

pub fn apply_general_chat_view_default(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    default_chat_view: bool,
) {
    if !default_chat_view {
        return;
    }
    let Some(tab_id) = mux.resolve_tab_id(GENERAL_ALIAS) else {
        return;
    };
    if let Some(tab) = mux.get_tab_mut(&tab_id) {
        tab.chat_view = true;
    }
}

/// The workspace holding the General tab — the surface the user watches by
/// default, and where a user-facing service tab must be raised.
pub fn general_workspace_id(mux: &crate::multiplexer::state::MultiplexerState) -> Option<String> {
    mux.resolve_tab_id(GENERAL_ALIAS)
        .and_then(|tab_id| mux.find_workspace_for_tab(&tab_id))
        .or_else(|| {
            mux.list_workspaces()
                .iter()
                .find(|workspace| workspace.protected)
                .map(|workspace| workspace.id.clone())
        })
}

/// Ensure General workspace + tab exist in MultiplexerState.
/// Called BEFORE mux is wrapped in Arc<Mutex<>> — no locking needed.
/// This avoids the deadlock that occurs when creating workspace from a background thread.
/// Returns how many duplicate protected workspaces were closed — a deliberate
/// reduction the caller must persist with authorized-reduction intent.
pub fn ensure_general_workspace(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    default_chat_view: bool,
) -> usize {
    let general_cwd = general_dir().to_string_lossy().to_string();

    let protected_ws: Vec<_> = mux
        .list_workspaces()
        .iter()
        .filter(|ws| ws.protected)
        .cloned()
        .collect();

    let mut duplicates_closed = 0usize;
    if protected_ws.len() > 1 {
        for ws in &protected_ws[1..] {
            let has_agent_tab = ws
                .tab_ids
                .iter()
                .any(|tab_id| mux.get_tab(tab_id).is_some_and(|tab| tab.agent_type.is_some()));
            if has_agent_tab {
                log::warn!(
                    "[general-agent] Leaving duplicate protected workspace {} because it contains agent tabs",
                    ws.id
                );
                continue;
            }
            mux.set_workspace_protected(&ws.id, false);
            if mux.close_workspace(&ws.id).is_ok() {
                duplicates_closed += 1;
            }
            log::info!(
                "[general-agent] Cleaned up duplicate protected workspace {}",
                ws.id
            );
        }
    }

    if !protected_ws.is_empty() {
        let ws_id = protected_ws[0].id.clone();
        if protected_ws[0].name != GENERAL_WORKSPACE_NAME {
            let _ = mux.rename_workspace(&ws_id, GENERAL_WORKSPACE_NAME);
        }
        mux.move_workspace_to_front(&ws_id);
        let tab_id = protected_ws[0]
            .tab_ids
            .iter()
            .find(|tab_id| {
                mux.get_tab(tab_id)
                    .map(|tab| !tab.hidden && tab.show_in_tabbar)
                    .unwrap_or(false)
            })
            .cloned()
            .or_else(|| match mux.create_tab(&ws_id) {
                Ok(tab_id) => {
                    let _ = mux.rename_tab(&tab_id, GENERAL_TAB_TITLE);
                    log::info!(
                        "[general-agent] Repaired empty General workspace {} with Tab {}",
                        ws_id,
                        tab_id
                    );
                    Some(tab_id)
                }
                Err(e) => {
                    log::error!("[general-agent] Failed to repair General tab: {}", e);
                    None
                }
            });
        if let Some(pid) = tab_id {
            let pid = pid.clone();
            let _ = mux.rename_tab(&pid, GENERAL_TAB_TITLE);
            if let Some(tab) = mux.get_tab_mut(&pid) {
                tab.protected = true;
                tab.team_id = None;
                super::identity::apply_general_agent_kind(tab);
                tab.cwd = Some(general_cwd.clone());
                tab.provisioned_cwd = Some(general_cwd);
                tab.hidden = false;
                tab.show_in_tabbar = true;
                if default_chat_view && !tab.chat_view {
                    tab.chat_view = true;
                }
            }
            mux.register_alias(GENERAL_ALIAS, &pid);
            mux.register_alias(MAIN_ALIAS, &pid);
            log::info!(
                "[general-agent] Existing General workspace, alias for Tab {}",
                pid
            );
        }
        return duplicates_closed;
    }

    let ws_info = mux.create_workspace(
        GENERAL_WORKSPACE_NAME.to_string(),
        Some(general_cwd.clone()),
    );
    mux.set_workspace_protected(&ws_info.id, true);
    mux.move_workspace_to_front(&ws_info.id);

    match mux.create_tab(&ws_info.id) {
        Ok(tab_id) => {
            let _ = mux.rename_tab(&tab_id, GENERAL_TAB_TITLE);
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.protected = true;
                tab.team_id = None;
                super::identity::apply_general_agent_kind(tab);
                tab.cwd = Some(general_cwd.clone());
                tab.provisioned_cwd = Some(general_cwd);
                tab.chat_view = default_chat_view;
            }
            mux.register_alias(GENERAL_ALIAS, &tab_id);
            mux.register_alias(MAIN_ALIAS, &tab_id);
            let _ = mux.set_active_workspace(&ws_info.id);
            let _ = mux.set_focused_tab(&tab_id);
            log::info!(
                "[general-agent] Created General workspace {} with Tab {}",
                ws_info.id,
                tab_id
            );
        }
        Err(e) => {
            log::error!("[general-agent] Failed to create General tab: {}", e);
        }
    }
    duplicates_closed
}

#[cfg(test)]
mod tests {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::general_agent::{GENERAL_TAB_TITLE, GENERAL_WORKSPACE_NAME};

    fn general_tab_chat_view(mux: &MultiplexerState) -> bool {
        let ws = mux
            .list_workspaces()
            .iter()
            .find(|ws| ws.name == GENERAL_WORKSPACE_NAME)
            .expect("general workspace")
            .clone();
        let tab_id = ws.tab_ids.first().expect("general Tab");
        mux.get_tab(tab_id).expect("tab").chat_view
    }

    fn assert_general_cwd_is_provisioned(mux: &MultiplexerState) {
        let tab_id = mux
            .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
            .expect("General alias");
        let tab = mux.get_tab(&tab_id).expect("General Tab");
        assert_eq!(tab.provisioned_cwd, tab.cwd);
        assert!(tab.provisioned_cwd.is_some());
    }

    #[test]
    fn general_workspace_created_with_default_chat_enabled_sets_chat_view() {
        let mut mux = MultiplexerState::empty();

        super::ensure_general_workspace(&mut mux, true);

        assert!(general_tab_chat_view(&mux));
        assert_general_cwd_is_provisioned(&mux);
    }

    #[test]
    fn general_workspace_created_with_default_chat_disabled_stays_terminal() {
        let mut mux = MultiplexerState::empty();

        super::ensure_general_workspace(&mut mux, false);

        assert!(!general_tab_chat_view(&mux));
    }

    #[test]
    fn general_workspace_repair_applies_default_to_existing_main_tab() {
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
        let tab_id = ws.tab_ids.first().expect("general Tab");
        let tab = mux.get_tab(tab_id).expect("tab");
        assert_eq!(tab.title, GENERAL_TAB_TITLE);
        assert!(tab.chat_view);
        assert_general_cwd_is_provisioned(&mux);
    }

    #[test]
    fn general_workspace_repair_creates_main_tab_when_protected_workspace_is_empty() {
        let mut mux = MultiplexerState::empty();
        let ws = mux.create_workspace(GENERAL_WORKSPACE_NAME.to_string(), None);
        mux.set_workspace_protected(&ws.id, true);

        super::ensure_general_workspace(&mut mux, true);

        let ws = mux
            .list_workspaces()
            .iter()
            .find(|ws| ws.name == GENERAL_WORKSPACE_NAME)
            .expect("general workspace")
            .clone();
        assert_eq!(ws.tab_ids.len(), 1);
        let tab = mux.get_tab(&ws.tab_ids[0]).expect("tab");
        assert_eq!(tab.title, GENERAL_TAB_TITLE);
        assert!(tab.protected);
        assert!(tab.chat_view);
    }

    #[test]
    fn general_workspace_does_not_adopt_a_hidden_service_tab() {
        let mut mux = MultiplexerState::empty();
        let ws = mux.create_workspace(GENERAL_WORKSPACE_NAME.to_string(), None);
        mux.set_workspace_protected(&ws.id, true);
        let service_tab_id = mux.create_tab(&ws.id).expect("service Tab");
        if let Some(tab) = mux.get_tab_mut(&service_tab_id) {
            tab.hidden = true;
            tab.show_in_tabbar = false;
        }

        super::ensure_general_workspace(&mut mux, true);

        let general_tab_id = mux
            .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
            .expect("General alias");
        assert_ne!(general_tab_id, service_tab_id);
        let general_tab = mux.get_tab(&general_tab_id).expect("General Tab");
        assert_eq!(general_tab.title, GENERAL_TAB_TITLE);
        assert!(!general_tab.hidden);
        assert!(general_tab.show_in_tabbar);
        let service_tab = mux.get_tab(&service_tab_id).expect("service Tab remains");
        assert!(service_tab.hidden);
        assert!(!service_tab.show_in_tabbar);
    }

    #[test]
    fn general_agent_chat_default_uses_provider_capability_not_bundled_id() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::with_chat_ui("community-chat"),
        ));
        let config = crate::config::schema::AppConfig::default();

        assert!(super::default_chat_view_for_provider(
            &config,
            &registry,
            "community-chat"
        ));
    }

    #[test]
    fn general_agent_chat_default_rejects_provider_without_chat_capability() {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            "community-terminal",
        )));
        let config = crate::config::schema::AppConfig::default();

        assert!(!super::default_chat_view_for_provider(
            &config,
            &registry,
            "community-terminal"
        ));
    }
}
