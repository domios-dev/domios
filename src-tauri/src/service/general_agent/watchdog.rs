//! General Agent watchdog — detects dead agent and triggers restart.

use super::{lifecycle, GENERAL_ALIAS};

/// Watchdog: periodically checks if the General Agent is alive.
/// If agent_type is None and shell prompt is visible, restarts the agent.
/// Respects `auto_restart`, `max_restart_attempts`, and `restart_delay_s` from config.
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
        // Wait for initial startup to complete
        std::thread::sleep(std::time::Duration::from_secs(20));
        let mut consecutive_restarts = 0u32;
        let check_interval = std::time::Duration::from_secs(10);

        loop {
            std::thread::sleep(check_interval);

            let general_config = config.lock().general_agent.clone();
            if !general_config.enabled || !general_config.auto_restart {
                continue;
            }

            // Find the General pane and check its state
            let needs_restart = {
                let m = mux.lock();
                m.resolve_pane_id(GENERAL_ALIAS).and_then(|pid| {
                    let pane = m.get_pane(&pid)?;
                    // In grace period — still launching, don't touch
                    let in_grace = pane.agent_set_at
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false);
                    if in_grace { return Some(false); }
                    // Check screen: shell prompt visible + no agent markers = needs restart
                    let (has_prompt, has_agent) = pane.pty_id.as_ref().and_then(|pty_id| {
                        screen_state.try_lock().and_then(|ss| {
                            ss.get_screen_text(pty_id).map(|screen| {
                                let prompt = screen.lines().rev()
                                    .find(|l| !l.trim().is_empty())
                                    .map(|l| {
                                        let t = l.trim();
                                        t.ends_with('>') || t.ends_with('$') || t.ends_with('%') || t.ends_with('#')
                                    })
                                    .unwrap_or(false);
                                let agent = screen.contains("Claude Code")
                                    || screen.contains("? for shortcuts")
                                    || screen.contains("Cogitated for")
                                    || screen.contains("Brewed for");
                                (prompt, agent)
                            })
                        })
                    }).unwrap_or((false, false));
                    if has_agent { return Some(false); }
                    Some(has_prompt)
                })
            };

            match needs_restart {
                Some(true) => {
                    if consecutive_restarts >= general_config.max_restart_attempts {
                        log::error!("[general-agent/watchdog] Max restarts ({}) reached — stopping",
                            general_config.max_restart_attempts);
                        break;
                    }
                    consecutive_restarts += 1;
                    log::info!("[general-agent/watchdog] Agent not running — restarting ({}/{})",
                        consecutive_restarts, general_config.max_restart_attempts);
                    // Clear stale agent_type so start_general_agent doesn't skip with "Already running"
                    let pid = mux.lock().resolve_pane_id(GENERAL_ALIAS);
                    if let Some(pid) = pid {
                        svc.set_pane_agent_type(&pid, None);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(general_config.restart_delay_s as u64));
                    lifecycle::start_general_agent(&svc, &mux, &pty, &screen_state, &config, &app);
                }
                Some(false) => {
                    consecutive_restarts = 0;
                }
                None => {
                    // No General pane found — nothing to do
                }
            }
        }
    });
}
