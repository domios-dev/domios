use super::ServiceContext;
use crate::agent::tab_state::{Surface, TabState, TabStateAction, TabStateClassification};
use codeterm_types::agent::provider_signals::{ApiErrorKind, PermissionMode, ProviderSignals};
use tauri::Manager;

mod incident;
use incident::{incident, incident_transition, IncidentTransition};

fn enabled(svc: &ServiceContext, provider: &str) -> bool {
    svc.app_handle
        .try_state::<crate::config::ConfigState>()
        .is_some_and(|config| {
            config.lock().agents.get(provider).is_some_and(|config| {
                ["structured_signals", "capability_probe_enabled"]
                    .iter()
                    .any(|key| config.extra.get(*key).and_then(toml::Value::as_bool) == Some(true))
            })
        })
}

pub(crate) fn classify_for(
    svc: &ServiceContext,
    snapshot: &crate::agent::tab_state::TabStateSnapshot,
    fallback: Option<TabStateClassification>,
) -> Option<TabStateClassification> {
    let signals = {
        let mux = svc.mux.read();
        mux.get_tab(&snapshot.tab_id)
            .filter(|tab| {
                tab.agent_type.as_deref() == Some(&snapshot.agent_type)
                    && tab.agent_session_id == snapshot.session_id
            })
            .map(|tab| tab.provider_signals.clone())
            .unwrap_or_default()
    };
    classify(&signals, fallback)
}

pub(crate) fn classify(
    signals: &ProviderSignals,
    fallback: Option<TabStateClassification>,
) -> Option<TabStateClassification> {
    let state = if signals.access_lost() {
        log::debug!("[provider-access] state=access_lost");
        Some(TabState::WaitingOnAuth)
    } else {
        match signals.api_error {
            Some(ApiErrorKind::Authentication) => None,
            Some(ApiErrorKind::RateLimit) => {
                log::debug!("[provider-access] state=rate_limited");
                Some(TabState::RateLimited)
            }
            Some(ApiErrorKind::Other) => {
                log::debug!("[provider-access] state=unknown");
                Some(TabState::Unknown)
            }
            None if signals.auth.as_ref().is_some_and(|auth| !auth.logged_in) => {
                Some(TabState::WaitingOnAuth)
            }
            None => None,
        }
    };
    if let Some(state) = state {
        return Some(TabStateClassification {
            state,
            confidence: 1.0,
            data: None,
            action: Some(TabStateAction {
                surface: Some(Surface {
                    kind: "provider_health".into(),
                    payload: serde_json::json!({"auth":signals.auth,"api_error":signals.api_error,"access_lost":signals.access_lost()}),
                }),
                ..Default::default()
            }),
        });
    }
    fallback.filter(|classification| {
        classification.state != TabState::WaitingOnAuth || signals.auth.is_none()
    })
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
    let structured = provider
        .as_deref()
        .is_some_and(|provider| enabled(svc, provider));
    let (changed, changed_error, refresh_needed, transition) = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab.agent_session_id.as_deref() != Some(session)
            || tab.session_binding_nonce.as_deref() != Some(nonce)
            || tab.agent_type != provider
        {
            log::debug!("[provider-access] rejected stale session tab={tab_id}");
            return;
        }
        let previous = tab.provider_signals.clone();

        observe_records(&mut tab.provider_signals, session, records, structured);
        let refresh_needed = previous.auth.as_ref().is_some_and(|auth| !auth.logged_in)
            && records.iter().any(|record| {
                if record
                    .provider_session_id
                    .as_deref()
                    .is_some_and(|id| id != session)
                {
                    return false;
                }
                record
                    .provider_observations
                    .as_ref()
                    .is_some_and(|observation| observation.clears_api_error)
            });
        (
            previous != tab.provider_signals,
            (previous.api_error != tab.provider_signals.api_error)
                .then_some(tab.provider_signals.api_error)
                .flatten(),
            refresh_needed,
            incident_transition(previous.access_lost(), tab.provider_signals.access_lost()),
        )
    };
    if transition != IncidentTransition::Unchanged {
        incident(svc, tab_id, session, nonce, transition);
    }
    if changed {
        svc.emit_state_changed();
    }
    if refresh_needed {
        refresh_auth(svc, tab_id);
    }
    if let Some(error) = changed_error {
        if error != ApiErrorKind::Authentication {
            warn(svc, tab_id, format!("Provider API error: {error:?}"));
        }
        if error == ApiErrorKind::Authentication {
            refresh_auth(svc, tab_id);
        }
    }
}

fn observe_records(
    signals: &mut ProviderSignals,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
    structured: bool,
) {
    for record in records {
        if record
            .provider_session_id
            .as_deref()
            .is_some_and(|id| id != session)
        {
            log::debug!("[provider-access] ignored foreign record");
            continue;
        }
        if let Some(observation) = &record.provider_observations {
            let mut observation = observation.clone();
            if !structured {
                observation.permission_mode = None;
            }
            signals.observe(&observation);
        }
    }
}

pub(crate) fn plan_drift(signals: &ProviderSignals) -> bool {
    signals.permission_mode == Some(PermissionMode::Plan)
}

pub(crate) fn warn(svc: &ServiceContext, tab: &str, body: String) {
    svc.create_notification_ext(
        "Provider state".into(),
        Some(body),
        Some("warning"),
        None,
        None,
        super::notification::NotificationOrigin::local_tab(tab.into()),
        super::notification::PushIntent::default(),
        None,
    );
}

pub(crate) fn warn_dispatch(svc: &ServiceContext, from_tab: &str, target_tab: &str) {
    let drift = {
        let mux = svc.mux.read();
        mux.get_tab(from_tab).is_some_and(|tab| {
            tab.role == Some(crate::multiplexer::types::AgentRole::Manager)
                && tab.role_kind_id()
                    == Some(codeterm_types::agent::role_kind::builtins::ORCHESTRATOR)
        }) && mux
            .get_tab(target_tab)
            .is_some_and(|tab| plan_drift(&tab.provider_signals))
    };
    if drift {
        warn(svc, from_tab, format!("Target tab {target_tab} is in plan mode; dispatched work may require a mode change."));
    }
}

pub(crate) fn refresh_auth(svc: &ServiceContext, tab_id: &str) {
    super::background_work_watch::ensure(svc, tab_id);
    super::capability_probe::refresh_tab(svc, tab_id);
    let launch = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .and_then(|tab| Some((tab.agent_type.clone()?, tab.session_binding_nonce.clone()?)))
    };
    let Some((provider_id, nonce)) = launch else {
        return;
    };
    if !enabled(svc, &provider_id) {
        return;
    }
    let request = svc
        .agent_registry
        .with_provider(&provider_id, |provider| {
            Some((
                provider.binary_name().to_string(),
                provider.auth_status_probe()?,
            ))
        })
        .flatten();
    let Some((binary, probe)) = request else {
        return;
    };
    {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab.session_binding_nonce.as_deref() != Some(&nonce)
            || tab.provider_signals.auth_probe_pending
        {
            return;
        }
        tab.provider_signals.auth_probe_pending = true;
    }
    let svc = svc.clone();
    let tab_id = tab_id.to_string();
    let failed_svc = svc.clone();
    let failed_tab = tab_id.clone();
    let failed_nonce = nonce.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("provider-auth-probe".into())
        .spawn(move || {
            let result = super::provider_signals_probe::capture(&binary, &probe);
            let auth = result.as_ref().and_then(|record| {
                svc.agent_registry
                    .with_provider(&provider_id, |provider| provider.parse_auth_status(record))
                    .flatten()
            });
            let enabled = enabled(&svc, &provider_id);
            let mut mux = svc.mux.write();
            let Some(tab) = mux.get_tab_mut(&tab_id) else {
                return;
            };
            if tab.agent_type.as_deref() != Some(&provider_id)
                || tab.session_binding_nonce.as_deref() != Some(&nonce)
            {
                return;
            }
            let next_auth = enabled.then_some(auth).flatten();
            let login_changed = tab.provider_signals.auth != next_auth;
            tab.provider_signals.auth = next_auth;
            tab.provider_signals.auth_probe_pending = false;
            drop(mux);
            svc.emit_state_changed();
            if login_changed {
                super::capability_probe::invalidate_and_rebuild(&provider_id);
            }
        })
    {
        if let Some(tab) = failed_svc.mux.write().get_tab_mut(&failed_tab) {
            if tab.session_binding_nonce.as_deref() == Some(&failed_nonce) {
                tab.provider_signals.auth_probe_pending = false;
            }
        }
        log::warn!("[provider-auth] probe could not start: {error}");
    }
}

#[cfg(test)]
mod tests;
