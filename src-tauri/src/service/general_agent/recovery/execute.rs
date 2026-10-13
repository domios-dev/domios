use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

use super::{decide, Action, Evidence, Label, OWNER_NOTIFICATION_CONFIDENCE};
use crate::service::supporter_tab_state::{ObservationTrigger, ShadowVerdict, TabStateMetadata};
use crate::service::ServiceContext;

#[derive(Default)]
struct Agreement {
    launch: String,
    episode: u64,
    previous: Option<(Label, f64, u64)>,
}

static AGREEMENT: LazyLock<Mutex<HashMap<String, Agreement>>> = LazyLock::new(Default::default);

fn agreement(
    tab: &str,
    launch: &str,
    episode: u64,
    observation: u64,
    label: Label,
    confidence: f64,
) -> (usize, f64) {
    let mut all = AGREEMENT.lock();
    let state = all.entry(tab.into()).or_default();
    if state.launch != launch || state.episode != episode {
        *state = Agreement {
            launch: launch.into(),
            episode,
            previous: None,
        };
    }
    if label == Label::Other || !(OWNER_NOTIFICATION_CONFIDENCE..=1.0).contains(&confidence) {
        state.previous = None;
        return (0, 0.0);
    }
    let result = match state.previous {
        Some((previous, value, id)) if previous == label && id != observation => {
            (2, value.min(confidence))
        }
        _ => (1, confidence),
    };
    state.previous = Some((label, confidence, observation));
    result
}

pub(crate) fn recover(
    service: &ServiceContext,
    tab_id: &str,
    metadata: &TabStateMetadata,
    episode: u64,
    observation_id: u64,
    _trigger: ObservationTrigger,
    verdict: &ShadowVerdict,
    ticket: Option<&crate::service::supporter_tab_state::coordinator::ActionTicket>,
) -> bool {
    if crate::service::agent_runtime::requested_for_tab(service, tab_id) {
        return true;
    }
    let label = Label::parse(&verdict.label);
    let (count, confidence) = agreement(
        tab_id,
        &metadata.session_key,
        episode,
        observation_id,
        label,
        verdict.confidence,
    );
    let current = { service.mux.read().get_tab(tab_id).cloned() };
    let Some(current) = current else {
        return false;
    };
    let Some(observed) = metadata.tab_snapshot.as_ref() else {
        return false;
    };
    if current.pty_id != observed.pty_id {
        return true;
    }
    let general = super::super::session_contract::owns_main(&service.mux.read(), observed);
    let connected = count >= 2;
    if !connected {
        return false;
    }
    let Some(ticket) = ticket else {
        return true;
    };
    if !crate::service::supporter_tab_state::coordinator::global().ticket_is_current(
        ticket,
        &crate::service::supporter_tab_state::coordinator::LedgerSnapshot::capture(service, tab_id),
    ) || !crate::service::supporter_tab_state::ShadowHost::observation_is_current(
        service, tab_id, metadata,
    ) {
        return true;
    }
    if label == Label::TrustOrPermission {
        match current.pending_prompt_kind {
            Some(codeterm_types::pty::PromptKind::DirectoryTrust) => {
                if service
                    .respond_to_directory_trust(tab_id, None)
                    .owns_prompt()
                    || service.deterministic_directory_trust_covers(tab_id)
                {
                    return true;
                }
            }
            Some(
                codeterm_types::pty::PromptKind::Provider
                | codeterm_types::pty::PromptKind::Structured,
            ) => {
                // The tab's permission responder owns this request; a model cannot approve it.
                return false;
            }
            _ => {}
        }
    }
    if !general {
        if !matches!(
            label,
            Label::ReadyIdle | Label::WorkingNormally | Label::Other
        ) {
            crate::service::supporter_notify::owner(
                service,
                tab_id,
                &verdict.label,
                "Inspect the tab and resolve its pending state.",
            );
        }
        return false;
    }
    let config = crate::config::load_config();
    let restart_episode = super::super::recovery_episode::store().snapshot();
    let pty_live = current
        .pty_id
        .as_deref()
        .is_some_and(|pty| service.pty.lock().get_session(pty).is_some());
    let provider_absent = metadata
        .process_tree_key
        .as_ref()
        .filter(|key| {
            key.tab_id == current.id
                && Some(key.pty_id.as_str()) == current.pty_id.as_deref()
                && (key.binding_nonce == current.session_binding_nonce
                    || current
                        .prior_session_binding_nonces
                        .last()
                        .is_some_and(|prior| Some(&prior.nonce) == key.binding_nonce.as_ref()))
        })
        .is_some_and(|key| {
            service.observe_provider_process_tree_for_key(key).state()
                == crate::service::process_tree::ProviderTreeState::GoneAfterSeen
        });
    let safe = current.pending_prompt.as_ref().and_then(|prompt| {
        current.agent_type.as_deref().and_then(|provider| {
            service
                .agent_registry
                .with_provider(provider, |provider| {
                    provider.update_prompt_safe_choice(prompt)
                })
                .flatten()
        })
    });
    let choice = verdict
        .safe_choice
        .as_ref()
        .map(|(value, _)| value.as_str());
    let e = Evidence {
        connected: true,
        agreeing_observations: count,
        minimum_confidence: confidence,
        automatic_actions_qualified: qualified(service, &config.supporter),
        pty_live,
        provider_absent,
        startup_deadline_elapsed: metadata
            .context
            .get("startup_deadline_elapsed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        prompt_kind: current.pending_prompt_kind,
        declared_safe_choice: safe.as_deref(),
        model_safe_choice: choice,
        safe_choice_confidence: verdict
            .safe_choice
            .as_ref()
            .map(|(_, confidence)| *confidence)
            .unwrap_or(0.0)
            .min(verdict.prompt_visible),
        directory_covered: service.deterministic_directory_trust_covers(tab_id),
        automatic_restarts: restart_episode.restarts,
        configured_restart_limit: config.general_agent.max_restart_attempts,
        restarted_for_label: restart_episode.restarted_for_label(&verdict.label),
        owner_required: restart_episode.owner_required,
    };
    let action = decide(label, &e);
    match action {
        Action::None | Action::PermissionPolicy => false,
        Action::DirectoryTrust => {
            service.respond_to_directory_trust(tab_id, None);
            true
        }
        Action::Notify => {
            crate::service::supporter_notify::owner(service, tab_id, &verdict.label, "Inspect the General Agent and resolve its pending state. Automatic recovery requires qualified model and runtime evidence.");
            true
        }
        Action::FailureNotification => {
            if super::super::recovery_episode::store().require_owner().ok() == Some(true) {
                crate::service::supporter_notify::failure(service, tab_id, "General Agent recovery stopped", "The restart budget is exhausted. Restart the General Agent to begin another episode.");
            }
            true
        }
        Action::AnswerSafeChoice(value) => {
            let Some(prompt) = current.pending_prompt.as_ref() else {
                return true;
            };
            if current.session_binding_nonce != observed.session_binding_nonce
                || Some(prompt) != observed.pending_prompt.as_ref()
            {
                return true;
            }
            let Some(index) = prompt
                .options
                .iter()
                .position(|option| option.value == value)
            else {
                return true;
            };
            let Some(identity) = current.prompt_identity() else {
                return true;
            };
            if !record_action(service, tab_id, verdict, "provider_declared_safe_choice") {
                return true;
            }
            match service.respond_prompt_with_identity(tab_id, &(index + 1).to_string(), &identity)
            {
                Ok(
                    crate::service::tab::RespondPromptOutcome::Sent
                    | crate::service::tab::RespondPromptOutcome::Structured,
                ) => crate::service::supporter_notify::verbose(
                    service,
                    tab_id,
                    "General Agent update choice",
                    "The provider option API accepted the declared safe choice.",
                ),
                Ok(_) => crate::service::supporter_notify::owner(
                    service,
                    tab_id,
                    &verdict.label,
                    "The update prompt changed before its safe choice could be answered.",
                ),
                Err(error) => crate::service::supporter_notify::owner(
                    service,
                    tab_id,
                    &verdict.label,
                    &format!("Safe-choice response failed: {error}"),
                ),
            }
            true
        }
        Action::RestartOnce => {
            if crate::crash::shutdown::is_shutting_down() || !config.general_agent.auto_restart {
                return true;
            }
            let key = format!("{}:{}:{episode}", metadata.session_key, verdict.label);
            if super::super::recovery_episode::store()
                .claim_restart(
                    &key,
                    &verdict.label,
                    config.general_agent.max_restart_attempts,
                )
                .ok()
                != Some(true)
            {
                return true;
            }
            let latest = { service.mux.read().get_tab(tab_id).cloned() };
            if !latest.is_some_and(|tab| {
                tab.pty_id == current.pty_id
                    && tab.session_binding_nonce == current.session_binding_nonce
                    && !tab.agent_booting
            }) {
                return true;
            }
            if !record_action(service, tab_id, verdict, "verified_provider_absence") {
                return true;
            }
            crate::service::supporter_notify::verbose(
                service,
                tab_id,
                "General Agent recovery",
                "Restarting with deterministic process evidence.",
            );
            if pty_live {
                if current.agent_type.is_some()
                    && !service.clear_tab_agent_identity_for_lifecycle_exit_with_cause(
                        tab_id,
                        "general_agent_recovery_provider_absence",
                    )
                {
                    crate::service::supporter_notify::owner(
                        service,
                        tab_id,
                        &verdict.label,
                        "Provider absence could not be revalidated before recovery.",
                    );
                    return true;
                }
                if let Err(error) = service.recover_general_agent_provider_exit(observed) {
                    crate::service::supporter_notify::owner(
                        service,
                        tab_id,
                        &verdict.label,
                        &format!("Verified-session recovery failed: {error:?}"),
                    );
                }
            } else {
                use tauri::Manager;
                if let Some(config) = service.app_handle.try_state::<crate::config::ConfigState>() {
                    let svc = std::sync::Arc::new(service.clone());
                    super::super::lifecycle::start_general_agent(
                        &svc,
                        &service.mux,
                        &service.pty,
                        &service.screen_state,
                        &config,
                        &service.app_handle,
                    );
                }
            }
            true
        }
    }
}

fn record_action(
    service: &ServiceContext,
    tab_id: &str,
    verdict: &ShadowVerdict,
    reason: &'static str,
) -> bool {
    let title = service
        .mux
        .read()
        .get_tab(tab_id)
        .map(|tab| tab.title.clone())
        .unwrap_or_else(|| tab_id.into());
    crate::service::supporter::runtime()
        .try_action(crate::service::supporter::ActionRecord {
            kind: crate::service::supporter::VerdictKind::TabState,
            tab: Some(tab_id),
            label: &verdict.label,
            confidence: verdict.confidence,
            action: crate::service::supporter::Action::Recovery,
            reason,
            asked: Some(crate::service::supporter::recovery_questions(
                &title, reason,
            )),
        })
        .is_ok()
}

fn qualified(service: &ServiceContext, config: &codeterm_types::config::SupporterConfig) -> bool {
    let Some(q) = super::qualification::recorded_qualification() else {
        return false;
    };
    let model = service
        .supporter_models
        .get(config.active_id().unwrap_or(""))
        .and_then(|model| model.model_id());
    q.authorizes(
        config.active_id().unwrap_or(""),
        model.as_deref(),
        &crate::service::supporter_tab_state::criteria_version(),
        &crate::service::supporter::criteria_hash(include_bytes!(
            "../../../../../evals/tab-state-jev/heldout.jsonl"
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ga_automatic_agreement_requires_both_observations_at_threshold() {
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            agreement(&id, "launch", 1, 1, Label::ErrorLoop, 0.80),
            (1, 0.80)
        );
        assert_eq!(
            agreement(&id, "launch", 1, 2, Label::ErrorLoop, 0.90),
            (2, 0.80)
        );
        assert_eq!(
            agreement(&id, "launch", 1, 3, Label::ErrorLoop, 0.90),
            (2, 0.90)
        );
        assert_eq!(agreement(&id, "launch", 1, 3, Label::ErrorLoop, 0.90).0, 1);
        assert_eq!(agreement(&id, "launch", 1, 4, Label::Other, 1.0).0, 0);
        assert_eq!(agreement(&id, "launch", 1, 5, Label::ErrorLoop, 0.90).0, 1);
        assert_eq!(
            agreement(&id, "new-launch", 1, 6, Label::ErrorLoop, 0.90).0,
            1
        );
    }
}
