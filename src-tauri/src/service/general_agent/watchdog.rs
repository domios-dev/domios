use super::{lifecycle, recovery_episode};
use crate::multiplexer::types::Tab;
use crate::service::ServiceContext;

#[derive(Clone, Copy, Debug)]
pub(crate) enum WatchdogEvent {
    PtyDeath,
    ProviderExit,
    LaunchSettlement,
}

struct EventEvidence {
    quiescing: bool,
    in_grace: bool,
    pty_is_live: bool,
    has_agent_type: bool,
    provider_absent: bool,
    enabled: bool,
    restarts: u32,
    restart_limit: u32,
    owner_required: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum EventAction {
    None,
    Restart,
    RestartShell,
    FailureNotification,
}

fn needs_restart_decision(
    quiescing: bool,
    in_grace: bool,
    pty_is_live: bool,
    has_agent_type: bool,
) -> bool {
    !quiescing && !in_grace && !pty_is_live && has_agent_type
}

fn event_decision(event: WatchdogEvent, e: &EventEvidence) -> EventAction {
    if !e.enabled || e.owner_required || e.quiescing {
        return EventAction::None;
    }
    let shell_exit = matches!(event, WatchdogEvent::ProviderExit)
        && e.pty_is_live
        && e.provider_absent
        && e.has_agent_type;
    let dead_pty = needs_restart_decision(e.quiescing, e.in_grace, e.pty_is_live, e.has_agent_type);
    if !shell_exit && !dead_pty {
        return EventAction::None;
    }
    if e.restarts >= e.restart_limit.min(super::recovery::MAX_AUTOMATIC_RESTARTS) {
        EventAction::FailureNotification
    } else if shell_exit {
        EventAction::RestartShell
    } else {
        EventAction::Restart
    }
}

impl ServiceContext {
    pub(crate) fn general_agent_watchdog_event(&self, tab: &Tab, event: WatchdogEvent) {
        self.general_agent_event(tab, event, None);
    }

    pub(crate) fn general_agent_provider_exit_event(
        &self,
        tab: &Tab,
        key: &crate::service::process_tree::ProviderTreeKey,
    ) {
        self.general_agent_event(tab, WatchdogEvent::ProviderExit, Some(key.clone()));
    }

    fn general_agent_event(
        &self,
        tab: &Tab,
        event: WatchdogEvent,
        tree_key: Option<crate::service::process_tree::ProviderTreeKey>,
    ) {
        if !super::session_contract::owns_main(&self.mux.read(), tab) {
            return;
        }
        let provider_absent = tree_key.as_ref().is_some_and(|key| {
            self.observe_provider_process_tree_for_key(key).state()
                == crate::service::process_tree::ProviderTreeState::GoneAfterSeen
        });
        let config = crate::config::load_config();
        let general = &config.general_agent;
        let episode = recovery_episode::store().snapshot();
        let evidence = EventEvidence {
            enabled: general.enabled && general.auto_restart && general.first_run_complete,
            quiescing: crate::crash::shutdown::is_shutting_down(),
            in_grace: tab
                .agent_set_at
                .is_some_and(|at| at.elapsed().as_secs() < 30),
            pty_is_live: tab
                .pty_id
                .as_deref()
                .is_some_and(|id| self.pty.lock().get_session(id).is_some()),
            has_agent_type: tab.agent_type.is_some(),
            provider_absent,
            restarts: episode.restarts,
            restart_limit: general.max_restart_attempts,
            owner_required: episode.owner_required,
        };
        let action = event_decision(event, &evidence);
        if action == EventAction::None {
            return;
        }
        let service = self.clone();
        let tab = tab.clone();
        std::thread::spawn(move || match action {
            EventAction::Restart | EventAction::RestartShell => {
                if crate::crash::shutdown::is_shutting_down() {
                    return;
                }
                let current = { service.mux.read().get_tab(&tab.id).cloned() };
                let Some(current) = current else {
                    return;
                };
                if current.pty_id != tab.pty_id || current.agent_booting {
                    return;
                }
                let live = tab
                    .pty_id
                    .as_deref()
                    .is_some_and(|id| service.pty.lock().get_session(id).is_some());
                let shell = action == EventAction::RestartShell;
                if shell {
                    let Some(tree_key) = tree_key.as_ref() else {
                        return;
                    };
                    if !live
                        || current.agent_type.is_some()
                        || current.session_binding_nonce.is_some()
                        || current
                            .prior_session_binding_nonces
                            .last()
                            .is_none_or(|prior| {
                                Some(&prior.nonce) != tree_key.binding_nonce.as_ref()
                            })
                        || service
                            .observe_provider_process_tree_for_key(tree_key)
                            .state()
                            != crate::service::process_tree::ProviderTreeState::GoneAfterSeen
                    {
                        return;
                    }
                } else if live {
                    return;
                }
                let key = format!(
                    "watchdog:{}:{:?}:{:?}",
                    tab.id, tab.pty_id, tab.session_binding_nonce
                );
                if recovery_episode::store()
                    .claim_restart(
                        &key,
                        if shell {
                            "crashed_to_shell"
                        } else {
                            "dead_pty"
                        },
                        general_limit(&service),
                    )
                    .ok()
                    != Some(true)
                {
                    return;
                }
                if crate::service::supporter::runtime()
                    .try_action(crate::service::supporter::ActionRecord {
                        kind: crate::service::supporter::VerdictKind::TabState,
                        tab: Some(&tab.id),
                        label: if shell {
                            "crashed_to_shell"
                        } else {
                            "dead_pty"
                        },
                        confidence: 1.0,
                        action: crate::service::supporter::Action::Recovery,
                        reason: if shell {
                            "confirmed_provider_exit_to_live_shell"
                        } else {
                            "watchdog_dead_pty_event"
                        },
                        asked: Some(crate::service::supporter::recovery_questions(
                            &tab.title,
                            "watchdog_dead_pty_event",
                        )),
                    })
                    .is_err()
                {
                    return;
                }
                crate::service::supporter_notify::verbose(
                    &service,
                    &tab.id,
                    "General Agent recovery",
                    if shell {
                        "Recovering after a confirmed provider exit to a live shell."
                    } else {
                        "Restarting after a confirmed PTY death."
                    },
                );
                if shell {
                    match service.recover_general_agent_provider_exit(&tab) {
                        Ok(outcome) => {
                            let reason = outcome.session_action();
                            let _ = crate::service::supporter::runtime().try_action(
                                crate::service::supporter::ActionRecord {
                                    kind: crate::service::supporter::VerdictKind::TabState,
                                    tab: Some(&tab.id),
                                    label: "crashed_to_shell",
                                    confidence: 1.0,
                                    action: crate::service::supporter::Action::Recovery,
                                    reason,
                                    asked: Some(crate::service::supporter::recovery_questions(
                                        &tab.title, reason,
                                    )),
                                },
                            );
                            crate::service::supporter_notify::verbose(
                                &service,
                                &tab.id,
                                "General Agent recovered",
                                reason,
                            );
                        }
                        Err(error) => crate::service::supporter_notify::owner(
                            &service,
                            &tab.id,
                            "crashed_to_shell",
                            &format!("Confirmed exit recovery failed: {error:?}"),
                        ),
                    }
                    return;
                }
                use tauri::Manager;
                let Some(config) = service.app_handle.try_state::<crate::config::ConfigState>()
                else {
                    return;
                };
                if crate::crash::shutdown::is_shutting_down() {
                    return;
                }
                let current = service.mux.read().get_tab(&tab.id).cloned();
                if !current.is_some_and(|current| current.pty_id == tab.pty_id)
                    || tab
                        .pty_id
                        .as_deref()
                        .is_some_and(|id| service.pty.lock().get_session(id).is_some())
                {
                    return;
                }
                let svc = std::sync::Arc::new(service.clone());
                lifecycle::start_general_agent(
                    &svc,
                    &service.mux,
                    &service.pty,
                    &service.screen_state,
                    &config,
                    &service.app_handle,
                );
            }
            EventAction::FailureNotification => {
                if recovery_episode::store().require_owner().ok() == Some(true) {
                    crate::service::supporter_notify::failure(&service, &tab.id, "General Agent stopped recovering", "The automatic restart limit was reached. Restart the General Agent to begin a new episode.");
                }
            }
            EventAction::None => {}
        });
    }
}

fn general_limit(_service: &ServiceContext) -> u32 {
    crate::config::load_config()
        .general_agent
        .max_restart_attempts
}

#[cfg(test)]
mod tests {
    use super::needs_restart_decision;

    #[test]
    fn grace_period_blocks_restart() {
        assert!(!needs_restart_decision(false, true, false, true));
        assert!(!needs_restart_decision(false, true, true, true));
    }

    #[test]
    fn cosmetic_agent_type_without_pty_triggers_restart() {
        assert!(needs_restart_decision(false, false, false, true));
    }

    #[test]
    fn no_pty_no_agent_type_does_not_restart() {
        assert!(!needs_restart_decision(false, false, false, false));
    }

    #[test]
    fn live_pty_never_authorizes_screen_based_restart() {
        assert!(!needs_restart_decision(false, false, true, true));
    }

    #[test]
    fn watchdog_does_not_restart_during_declared_quiesce() {
        // The update handoff reaps the general agent's PTY on purpose.
        assert!(
            !needs_restart_decision(true, false, false, true),
            "a declared quiesce must not be restarted into"
        );
        // Narrowed, not disabled: the same dead process outside a quiesce still restarts.
        assert!(
            needs_restart_decision(false, false, false, true),
            "outside a declared quiesce the watchdog still restarts a dead agent"
        );
    }
}

#[cfg(test)]
mod event_tests;
