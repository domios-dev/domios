use super::*;

fn evidence() -> EventEvidence {
    EventEvidence {
        quiescing: false,
        in_grace: false,
        pty_is_live: false,
        has_agent_type: true,
        connected: false,
        enabled: true,
        restarts: 0,
        restart_limit: 7,
        owner_required: false,
    }
}

#[test]
fn ga_watchdog_restarts_on_pty_death_event_without_polling() {
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &evidence()), EventAction::Restart);
    let mut e = evidence();
    e.in_grace = true;
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e), EventAction::None);
    e.in_grace = false;
    e.quiescing = true;
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e), EventAction::None);
}

#[test]
fn ga_no_adapter_matches_todays_watchdog() {
    let mut e = evidence();
    for live in [false, true] {
        for has_type in [false, true] {
            for grace in [false, true] {
                for quiescing in [false, true] {
                    e.pty_is_live = live;
                    e.has_agent_type = has_type;
                    e.in_grace = grace;
                    e.quiescing = quiescing;
                    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e) == EventAction::Restart,
                        needs_restart_decision(quiescing, grace, live, has_type));
                }
            }
        }
    }
}

#[test]
fn ga_watchdog_live_shell_and_connected_adapter_require_recovery_policy() {
    let mut e = evidence();
    e.pty_is_live = true;
    assert_eq!(event_decision(WatchdogEvent::ProviderExit, &e), EventAction::None);
    e.pty_is_live = false;
    e.connected = true;
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e), EventAction::None);
}

#[test]
fn ga_watchdog_never_resets_restart_count_on_cosmetic_healthy_event() {
    let mut e = evidence();
    e.restarts = 2;
    e.pty_is_live = true;
    assert_eq!(event_decision(WatchdogEvent::LaunchSettlement, &e), EventAction::None);
    e.pty_is_live = false;
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e), EventAction::FailureNotification);
    e.owner_required = true;
    assert_eq!(event_decision(WatchdogEvent::PtyDeath, &e), EventAction::None);
}
