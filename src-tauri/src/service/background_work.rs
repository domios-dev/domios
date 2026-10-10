use super::ServiceContext;
use codeterm_types::agent::background_work::{BackgroundWorkObservation, BackgroundWorkState};
use tauri::Manager;

pub(crate) fn restart_guard(
    items: Vec<String>,
    force: bool,
) -> Result<(), super::agent_restart::AgentRestartError> {
    if items.is_empty() || force {
        Ok(())
    } else {
        Err(super::agent_restart::AgentRestartError::BackgroundWork { items })
    }
}

pub(crate) fn running(svc: &ServiceContext, tab_id: &str) -> Vec<String> {
    let provider = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let Some(provider) = provider else {
        return Vec::new();
    };
    let enabled = svc
        .app_handle
        .try_state::<crate::config::ConfigState>()
        .is_some_and(|config| {
            config
                .lock()
                .agents
                .get(&provider)
                .and_then(|config| config.extra.get("background_work_guard"))
                .and_then(toml::Value::as_bool)
                == Some(true)
        });
    if !enabled
        || !svc
            .agent_registry
            .with_provider(&provider, |provider| provider.tracks_background_work())
            .unwrap_or(false)
    {
        return Vec::new();
    }
    svc.mux
        .read()
        .get_tab(tab_id)
        .filter(|tab| tab.agent_type.as_deref() == Some(&provider))
        .map(|tab| tab.provider_signals.background.guarded_items())
        .unwrap_or_default()
}

pub(crate) fn apply(
    svc: &ServiceContext,
    tab_id: &str,
    session: &str,
    nonce: &str,
    records: &[crate::agent::chat_types::RichMessage],
) {
    let provider = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let Some(provider) = provider else {
        return;
    };
    if !svc
        .agent_registry
        .with_provider(&provider, |provider| provider.tracks_background_work())
        .unwrap_or(false)
    {
        return;
    }
    let changed = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab.agent_type.as_deref() != Some(&provider)
            || tab.agent_session_id.as_deref() != Some(session)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
        {
            return;
        }
        let before = tab.provider_signals.background.clone();
        for record in records {
            if record
                .provider_session_id
                .as_deref()
                .is_some_and(|id| id != session)
            {
                continue;
            }
            if tab.fork_source_session_id.is_some()
                && record.provider_session_id.as_deref() != Some(session)
            {
                continue;
            }
            for block in &record.blocks {
                if let Some(id) = block
                    .data
                    .get("tool_background_task_id")
                    .and_then(serde_json::Value::as_str)
                {
                    let label = block
                        .data
                        .get("tool_summary")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    tab.provider_signals.background.observe(
                        &BackgroundWorkObservation {
                            id: id.into(),
                            state: BackgroundWorkState::Running,
                        },
                        label,
                    );
                }
            }
            for observation in &record.background_work {
                tab.provider_signals.background.observe(observation, "");
            }
        }
        before != tab.provider_signals.background
    };
    if changed {
        svc.emit_state_changed();
        super::provider_update::background_work_changed(svc, tab_id);
    }
}

pub(crate) fn force_notice(svc: &ServiceContext, tab: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    log::warn!("[restart-guard] explicit force override tab={tab} running={items:?}");
    let owner = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.parent_tab_id.clone())
        .unwrap_or_else(|| tab.into());
    super::provider_signals::warn(
        svc,
        &owner,
        format!(
            "Forced restart of tab {tab} overrides running background work: {}",
            items.join(", ")
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_guard_refuses_named_work_and_allows_explicit_force() {
        let items = vec!["shell-fixture".into(), "agent-fixture".into()];
        assert!(
            matches!(restart_guard(items.clone(),false),Err(super::super::agent_restart::AgentRestartError::BackgroundWork{items:running}) if running==items)
        );
        assert!(restart_guard(items, true).is_ok());
        assert!(restart_guard(Vec::new(), false).is_ok());
    }
}

pub(crate) fn incomplete_snapshot(svc: &ServiceContext, tab_id: &str, session: &str, nonce: &str) {
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| {
        tab.agent_session_id.as_deref() == Some(session)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
    }) else {
        return;
    };
    if !tab.provider_signals.background.history_initialized {
        tab.provider_signals.background.history_incomplete = true;
    }
    drop(mux);
    svc.emit_state_changed();
}
