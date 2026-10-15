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

pub fn apply(
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
    let matches_binding = |tab: &crate::multiplexer::types::Tab| {
        tab.agent_type.as_deref() == Some(&provider)
            && tab.agent_session_id.as_deref() == Some(session)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
    };
    for _ in 0..2 {
        let (before, fork_source) = {
            let mux = svc.mux.read();
            let Some(tab) = mux.get_tab(tab_id).filter(|tab| matches_binding(tab)) else {
                return;
            };
            (
                tab.provider_signals.background.clone(),
                tab.fork_source_session_id.clone(),
            )
        };
        let mut next = observe_background_records(&before, fork_source.is_some(), session, records);
        let changed = next != before;
        let committed = {
            let mut mux = svc.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            if matches_binding(tab)
                && tab.fork_source_session_id == fork_source
                && tab.provider_signals.background == before
            {
                std::mem::swap(&mut tab.provider_signals.background, &mut next);
                true
            } else {
                false
            }
        };
        if committed {
            if changed {
                svc.emit_state_changed();
                super::provider_update::background_work_changed(svc, tab_id);
            }
            return;
        }
    }
    log::debug!("[background-work] discarded contended update tab={tab_id}");
}

fn observe_background_records(
    previous: &codeterm_types::agent::background_work::BackgroundRoster,
    forked: bool,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
) -> codeterm_types::agent::background_work::BackgroundRoster {
    let mut roster = previous.clone();
    for record in records {
        if record
            .provider_session_id
            .as_deref()
            .is_some_and(|id| id != session)
        {
            continue;
        }
        if forked && record.provider_session_id.as_deref() != Some(session) {
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
                roster.observe(
                    &BackgroundWorkObservation {
                        id: id.into(),
                        state: BackgroundWorkState::Running,
                    },
                    label,
                );
            }
        }
        for observation in &record.background_work {
            roster.observe(observation, "");
        }
    }
    roster
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
mod tests;

pub fn incomplete_snapshot(svc: &ServiceContext, tab_id: &str, session: &str, nonce: &str) {
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
