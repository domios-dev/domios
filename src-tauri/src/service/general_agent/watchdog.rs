//! General Agent watchdog — detects dead agent and triggers restart.

use super::{lifecycle, GENERAL_ALIAS};

fn needs_restart_decision(
    in_grace: bool,
    has_pty: bool,
    has_agent_type: bool,
    screen: Option<&str>,
) -> bool {
    if in_grace {
        return false;
    }
    if !has_pty && has_agent_type {
        return true;
    }
    let Some(text) = screen else {
        return false;
    };
    let agent_visible = text.contains("Claude Code")
        || text.contains("? for shortcuts")
        || text.contains("Cogitated for")
        || text.contains("Brewed for");
    if agent_visible {
        return false;
    }
    let has_prompt = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| {
            let t = l.trim();
            t.ends_with('>') || t.ends_with('$') || t.ends_with('%') || t.ends_with('#')
        })
        .unwrap_or(false);
    has_prompt
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
                    let in_grace = pane
                        .agent_set_at
                        .map(|t| t.elapsed().as_secs() < 30)
                        .unwrap_or(false);
                    let screen = pane.pty_id.as_ref().and_then(|pty_id| {
                        screen_state
                            .try_lock()
                            .and_then(|ss| ss.get_screen_text(pty_id))
                    });
                    Some(needs_restart_decision(
                        in_grace,
                        pane.pty_id.is_some(),
                        pane.agent_type.is_some(),
                        screen.as_deref(),
                    ))
                })
            };

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
                    // Clear stale agent_type so start_general_agent doesn't skip with "Already running"
                    let pid = mux.lock().resolve_pane_id(GENERAL_ALIAS);
                    if let Some(pid) = pid {
                        svc.set_pane_agent_type(&pid, None);
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
        assert!(!needs_restart_decision(true, false, true, None));
        assert!(!needs_restart_decision(true, true, true, Some("PS C:\\>")));
    }

    #[test]
    fn cosmetic_agent_type_without_pty_triggers_restart() {
        assert!(needs_restart_decision(false, false, true, None));
    }

    #[test]
    fn no_pty_no_agent_type_does_not_restart() {
        assert!(!needs_restart_decision(false, false, false, None));
    }

    #[test]
    fn agent_visible_blocks_restart() {
        let screen = "Welcome to Claude Code\nReady\n";
        assert!(!needs_restart_decision(false, true, true, Some(screen)));
    }

    #[test]
    fn agent_marker_shortcuts_blocks_restart() {
        let screen = "Some agent UI\n? for shortcuts\n";
        assert!(!needs_restart_decision(false, true, true, Some(screen)));
    }

    #[test]
    fn bare_powershell_prompt_with_cosmetic_agent_type_triggers_restart() {
        let screen =
            "Windows PowerShell\nCopyright (C) Microsoft Corporation.\n\nPS C:\\Users\\foo>";
        assert!(needs_restart_decision(false, true, true, Some(screen)));
    }

    #[test]
    fn bash_prompt_with_no_agent_triggers_restart() {
        let screen = "user@host:~$";
        assert!(needs_restart_decision(false, true, true, Some(screen)));
    }

    #[test]
    fn empty_screen_does_not_restart() {
        assert!(!needs_restart_decision(false, true, true, Some("")));
    }

    #[test]
    fn no_prompt_no_agent_does_not_restart() {
        let screen = "loading...\nstill loading\n";
        assert!(!needs_restart_decision(false, true, true, Some(screen)));
    }

    #[test]
    fn last_line_only_drives_prompt_detection() {
        let screen = "old prompt $\nclaude is running now\nno prompt suffix here";
        assert!(!needs_restart_decision(false, true, true, Some(screen)));
    }
}
