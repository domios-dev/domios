//! Verified shell-spawn and guarded agent-input injection.
use super::build_spawn_dispatch;
use crate::service::ServiceContext;

impl ServiceContext {
    fn write_spawn_dispatch(&self, pty_id: &str, cmd: &str) -> Result<usize, String> {
        let bytes = build_spawn_dispatch(cmd).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            let signal = self.screen_state.lock().arm_screen_change_signal(pty_id);
            let generation = signal.generation();
            self.pty
                .lock()
                .write(pty_id, &bytes)
                .map_err(|e| e.to_string())?;
            signal.wait_after(generation, super::PASTE_RENDER_WINDOW);
            self.pty
                .lock()
                .write(pty_id, b"\r")
                .map_err(|e| e.to_string())?;
            return Ok(bytes.len() + 1);
        }
        #[cfg(not(windows))]
        {
            self.pty
                .lock()
                .write(pty_id, &bytes)
                .map_err(|e| e.to_string())?;
            Ok(bytes.len())
        }
    }

    pub fn send_shell_spawn_after_ready(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        self.write_spawn_dispatch(pty_id, cmd).map(|_| ())
    }

    /// Launch a shell-level command in an attached pty, bypassing bracketed paste
    /// (see `build_spawn_dispatch` — bash 3.2 can't interpret paste markers).
    pub fn send_shell_spawn(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        self.await_shell_ready(pty_id);
        self.write_spawn_dispatch(pty_id, cmd).map(|_| ())
    }

    pub(crate) fn send_text_verified_guarded(
        &self,
        pty_id: &str,
        text: &str,
        enter: bool,
        submit_strategy: crate::agent::provider::VerifySubmitStrategy,
        permit: crate::pty::write_queue::GuardedInputPermit,
        opts: crate::service::inject_verify::VerifyOpts,
    ) -> Result<
        crate::service::inject_verify::InjectionResult,
        crate::service::inject_verify::InjectionError,
    > {
        super::submit_transaction::submit_text_with_key(
            self,
            pty_id,
            text,
            enter,
            b"\r",
            submit_strategy,
            opts,
            Some(permit),
        )
    }

    /// Inject a command into a plain shell tab via script-exec, bypassing
    /// bracketed-paste. Ensures the PTY exists and awaits the PTY link before
    /// writing; delegates to `send_shell_spawn` which calls `await_shell_ready`.
    pub fn send_shell_cmd(&self, tab_id: &str, cmd: &str) -> Result<(), String> {
        self.ensure_tab_pty_for_send(tab_id)?;
        let pty_id = self.await_pty_link_blocking(tab_id, std::time::Duration::from_secs(15))?;
        self.send_shell_spawn(&pty_id, cmd)
    }

    /// Shell-level agent launch with submit verification via [`build_spawn_dispatch`]
    /// (so bash 3.2 never sees raw ESC markers). Echo verify is skipped (the screen
    /// shows `bash /tmp/...`); for `TuiInputDrained`, an event-driven `agent_signal`
    /// condvar blocks until real TUI glyphs appear — no timing race, no sleep.
    pub fn send_shell_spawn_verified(
        &self,
        pty_id: &str,
        agent_id: &str,
        cmd: &str,
        submit_strategy: crate::agent::provider::VerifySubmitStrategy,
        opts: crate::service::inject_verify::VerifyOpts,
    ) -> Result<
        crate::service::inject_verify::InjectionResult,
        crate::service::inject_verify::InjectionError,
    > {
        use crate::agent::provider::VerifySubmitStrategy;
        use crate::service::inject_verify::{
            snapshot_screen_hash, wait_for_submit_shell, HelperErr, InjectionError, InjectionResult,
        };

        let started = std::time::Instant::now();

        // Gate on shell-prompt readiness before writing (Windows ConPTY first-byte race) — see `await_shell_ready`.
        self.await_shell_ready(pty_id);

        // Guard footer-only TUI frames that look like bare shell prompts.
        {
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            if crate::service::agent_restart::screen_shows_agent_tui(&self.agent_registry, &screen)
            {
                log::warn!(
                    "[inject] agent TUI on screen for pty {} - refusing shell-spawn launch write",
                    &pty_id[..pty_id.len().min(8)]
                );
                return Err(
                    crate::service::inject_verify::InjectionError::AgentTuiPresent {
                        pty_id: pty_id.to_string(),
                    },
                );
            }
        }

        self.feed_input_parser(pty_id, cmd, true);

        let hash_before = snapshot_screen_hash(&self.screen_state, pty_id).unwrap_or_default();

        let bytes_written =
            self.write_spawn_dispatch(pty_id, cmd)
                .map_err(|_| InjectionError::PtyClosed {
                    pty_id: pty_id.to_string(),
                })?;

        let submit_observed = if opts.verify_submit {
            let res: Result<(), HelperErr> = match submit_strategy {
                VerifySubmitStrategy::ShellPrompt { timeout } => {
                    wait_for_submit_shell(&self.screen_state, pty_id, hash_before, timeout, 8)
                        .map(|_| ())
                }
                VerifySubmitStrategy::TuiInputDrained { .. } => {
                    if self.await_agent_tui_frame_on_screen(pty_id, agent_id, opts.submit_timeout) {
                        Ok(())
                    } else {
                        log::debug!(
                            "agent signal timed out for pty {} (shell-dispatch path); proceeding",
                            pty_id
                        );
                        Err(HelperErr::Timeout(String::new()))
                    }
                }
                VerifySubmitStrategy::None => Err(HelperErr::Timeout(
                    self.screen_state
                        .lock()
                        .tail_text(pty_id, 8)
                        .unwrap_or_default(),
                )),
            };
            match res {
                Ok(()) => true,
                Err(HelperErr::PtyUnknown(id)) => {
                    return Err(InjectionError::PtyClosed { pty_id: id });
                }
                Err(HelperErr::Timeout(screen_tail)) => {
                    return Err(InjectionError::SubmitNeverConfirmed {
                        waited: started.elapsed(),
                        screen_tail,
                    });
                }
            }
        } else {
            false
        };

        Ok(InjectionResult {
            bytes_written,
            echo_observed: false,
            submit_observed,
            submit_anomaly: false,
            submit_key_writes: usize::from(cfg!(windows)),
            elapsed: started.elapsed(),
        })
    }
}
