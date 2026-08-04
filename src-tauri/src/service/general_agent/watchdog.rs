//! General Agent watchdog — detects dead agent and triggers restart.

use super::{lifecycle, GENERAL_ALIAS};

fn needs_restart_decision(in_grace: bool, pty_is_live: bool, has_agent_type: bool) -> bool {
    !in_grace && !pty_is_live && has_agent_type
}

pub fn start_watchdog(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &tauri::AppHandle,
) {
    let svc = svc.clone();
    let mux = mux.clone();
    let pty = pty.clone();
    let screen_state = screen_state.clone();
    let config = config.clone();
    let app = app.clone();

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(20));
        let mut consecutive_restarts = 0u32;
        let check_interval = std::time::Duration::from_secs(10);

        loop {
            std::thread::sleep(check_interval);

            let general_config = config.lock().general_agent.clone();
            if !general_config.enabled
                || !general_config.auto_restart
                || !general_config.first_run_complete
            {
                continue;
            }

            let tab_liveness = {
                let m = mux.write();
                m.resolve_tab_id(GENERAL_ALIAS).and_then(|pid| {
                    let tab = m.get_tab(&pid)?;
                    let in_grace = tab
                        .agent_set_at
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false);
                    Some((in_grace, tab.pty_id.clone(), tab.agent_type.is_some()))
                })
            };
            let needs_restart = tab_liveness.map(|(in_grace, pty_id, has_agent_type)| {
                let pty_is_live = pty_id
                    .as_deref()
                    .is_some_and(|id| pty.lock().get_session(id).is_some());
                needs_restart_decision(in_grace, pty_is_live, has_agent_type)
            });

            match needs_restart {
                Some(true) => {
                    if consecutive_restarts >= general_config.max_restart_attempts {
                        log::error!(
                            "[general-agent/watchdog] Max restarts ({}) reached — stopping",
                            general_config.max_restart_attempts
                        );
                        break;
                    }
                    consecutive_restarts += 1;
                    log::info!(
                        "[general-agent/watchdog] Agent not running — restarting ({}/{})",
                        consecutive_restarts,
                        general_config.max_restart_attempts
                    );
                    let pid = mux.write().resolve_tab_id(GENERAL_ALIAS);
                    if let Some(pid) = pid {
                        svc.clear_tab_agent_identity_for_lifecycle_exit(&pid);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(
                        general_config.restart_delay_s as u64,
                    ));
                    lifecycle::start_general_agent(&svc, &mux, &pty, &screen_state, &config, &app);
                }
                Some(false) => {
                    consecutive_restarts = 0;
                }
                None => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::needs_restart_decision;

    #[test]
    fn grace_period_blocks_restart() {
        assert!(!needs_restart_decision(true, false, true));
        assert!(!needs_restart_decision(true, true, true));
    }

    #[test]
    fn cosmetic_agent_type_without_pty_triggers_restart() {
        assert!(needs_restart_decision(false, false, true));
    }

    #[test]
    fn no_pty_no_agent_type_does_not_restart() {
        assert!(!needs_restart_decision(false, false, false));
    }

    #[test]
    fn live_pty_never_authorizes_screen_based_restart() {
        assert!(!needs_restart_decision(false, true, true));
    }
}
