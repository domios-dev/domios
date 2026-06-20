//! Verified injection path: send_shell_spawn, send_text_verified,
//! send_shell_spawn_verified.
use super::{build_spawn_dispatch, poll_shell_prompt};
use crate::service::ServiceContext;

impl ServiceContext {
    /// Launch a shell-level command in an attached pty, bypassing bracketed paste
    /// (see `build_spawn_dispatch` — bash 3.2 can't interpret paste markers).
    pub fn send_shell_spawn(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        self.await_shell_ready(pty_id);
        let bytes = build_spawn_dispatch(cmd).map_err(|e| e.to_string())?;
        self.pty
            .lock()
            .write(pty_id, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Verified text injection. Holds no ScreenState lock across helper calls
    /// (callers must not hold mux / output_buf). Delivery is best-effort: when
    /// `enter` is true, Enter fires unconditionally after the paste — echo/submit
    /// checks are reporting fields, not gates, so a verify timeout never skips
    /// Enter (the old bug that stranded the recipient in paste-mode).
    pub fn send_text_verified(
        &self,
        pty_id: &str,
        text: &str,
        enter: bool,
        submit_strategy: crate::agent::provider::VerifySubmitStrategy,
        opts: crate::service::inject_verify::VerifyOpts,
    ) -> Result<
        crate::service::inject_verify::InjectionResult,
        crate::service::inject_verify::InjectionError,
    > {
        use crate::agent::provider::VerifySubmitStrategy;
        use crate::service::inject_verify::{
            snapshot_screen_hash, wait_for_echo, wait_for_submit_shell,
            wait_for_submit_tui_drained, HelperErr, InjectionError, InjectionResult,
        };
        let started = std::time::Instant::now();
        let tail_rows: usize = 8;
        // Notify, never block — an unsafe-prompt detection must not lock the user out of sending.
        super::super::inject_safety::notify_if_unsafe_prompt(&self.screen_state, pty_id);

        if opts.require_prompt_first {
            let ready = poll_shell_prompt(&self.screen_state, pty_id, opts.prompt_timeout);
            if !ready {
                return Err(InjectionError::PromptNeverReady {
                    waited: started.elapsed(),
                });
            }
        }

        let hash_before = snapshot_screen_hash(&self.screen_state, pty_id).unwrap_or_default();

        self.feed_input_parser(pty_id, text, enter);

        // Empty text is a bare submit: skip the paste (avoids a stray space on Windows) and just press Enter below.
        let bytes_written = if text.is_empty() {
            0
        } else {
            let paste = if cfg!(windows) {
                format!("\x1b[200~ {}\x1b[201~", text)
            } else {
                format!("\x1b[200~{}\x1b[201~", text)
            };
            self.pty
                .lock()
                .write(pty_id, paste.as_bytes())
                .map_err(|_| InjectionError::PtyClosed {
                    pty_id: pty_id.to_string(),
                })?;
            paste.len()
        };

        // Echo verify is informational: a timeout means unconfirmed, not failed. Only PtyUnknown is a real failure.
        let echo_observed = if opts.verify_echo {
            match wait_for_echo(
                &self.screen_state,
                pty_id,
                text,
                opts.echo_timeout,
                tail_rows,
            ) {
                Ok(_) => true,
                Err(HelperErr::PtyUnknown(id)) => {
                    return Err(InjectionError::PtyClosed { pty_id: id });
                }
                Err(HelperErr::Timeout(_)) => {
                    log::debug!(
                        "echo verify timed out for pty {} (text len={}); proceeding to submit Enter anyway",
                        pty_id,
                        text.len()
                    );
                    false
                }
            }
        } else {
            false
        };

        // A TUI must finish ingesting the paste before Enter, else the `\r` is swallowed as part of it.
        // For TuiInputDrained providers without an echo wait, block (bounded) until the paste appears.
        if enter
            && !text.is_empty()
            && !opts.verify_echo
            && matches!(
                &submit_strategy,
                VerifySubmitStrategy::TuiInputDrained { .. }
            )
        {
            let _ = wait_for_echo(
                &self.screen_state,
                pty_id,
                text,
                opts.echo_timeout.max(std::time::Duration::from_secs(2)),
                tail_rows,
            );
        }

        // Enter always fires when asked — load-bearing: no early-return above may skip it unless the PTY is gone.
        let submit_observed = if enter {
            // Escalating retries: a `\r` mid-ingest is swallowed, so re-send with a longer settle. Capped at 2 (avoids minute-long stalls).
            const SETTLE_MS: [u64; 2] = [120, 800];
            let mut observed = false;
            for (attempt, settle_ms) in SETTLE_MS.iter().enumerate() {
                std::thread::sleep(std::time::Duration::from_millis(*settle_ms));
                self.pty
                    .lock()
                    .write(pty_id, b"\r")
                    .map_err(|_| InjectionError::PtyClosed {
                        pty_id: pty_id.to_string(),
                    })?;
                if !opts.verify_submit {
                    break;
                }
                let res = match &submit_strategy {
                    VerifySubmitStrategy::ShellPrompt { timeout } => wait_for_submit_shell(
                        &self.screen_state,
                        pty_id,
                        hash_before,
                        *timeout,
                        tail_rows,
                    ),
                    VerifySubmitStrategy::TuiInputDrained {
                        timeout,
                        idle_after_drain,
                    } => wait_for_submit_tui_drained(
                        &self.screen_state,
                        pty_id,
                        text,
                        *timeout,
                        *idle_after_drain,
                        tail_rows,
                    ),
                    VerifySubmitStrategy::None => Ok(String::new()),
                };
                match res {
                    Ok(_) => {
                        observed = true;
                        break;
                    }
                    Err(HelperErr::PtyUnknown(id)) => {
                        return Err(InjectionError::PtyClosed { pty_id: id });
                    }
                    Err(HelperErr::Timeout(_)) => {
                        log::info!(
                            "[inject] submit verify timed out for pty {} (attempt {}/{}); re-sending Enter",
                            &pty_id[..pty_id.len().min(8)],
                            attempt + 1,
                            SETTLE_MS.len(),
                        );
                    }
                }
            }
            observed
        } else {
            false
        };

        Ok(InjectionResult {
            bytes_written,
            echo_observed,
            submit_observed,
            elapsed: started.elapsed(),
        })
    }

    /// Shell-level agent launch with submit verification via [`build_spawn_dispatch`]
    /// (so bash 3.2 never sees raw ESC markers). Echo verify is skipped (the screen
    /// shows `bash /tmp/...`); for `TuiInputDrained`, an event-driven `agent_signal`
    /// condvar blocks until real TUI glyphs appear — no timing race, no sleep.
    pub fn send_shell_spawn_verified(
        &self,
        pty_id: &str,
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

        self.feed_input_parser(pty_id, cmd, true);

        let hash_before = snapshot_screen_hash(&self.screen_state, pty_id).unwrap_or_default();

        // Arm the agent signal BEFORE the write so we cannot miss an instant startup.
        let agent_signal = self.screen_state.lock().arm_agent_signal(pty_id);

        let bytes = build_spawn_dispatch(cmd).map_err(|_| InjectionError::PtyClosed {
            pty_id: pty_id.to_string(),
        })?;
        self.pty
            .lock()
            .write(pty_id, &bytes)
            .map_err(|_| InjectionError::PtyClosed {
                pty_id: pty_id.to_string(),
            })?;
        let bytes_written = bytes.len();

        let submit_observed = if opts.verify_submit {
            let res: Result<(), HelperErr> = match submit_strategy {
                VerifySubmitStrategy::ShellPrompt { timeout } => {
                    wait_for_submit_shell(&self.screen_state, pty_id, hash_before, timeout, 8)
                        .map(|_| ())
                }
                VerifySubmitStrategy::TuiInputDrained { timeout, .. } => {
                    if agent_signal.wait(timeout) {
                        Ok(())
                    } else {
                        log::debug!(
                            "agent signal timed out for pty {} (shell-dispatch path); proceeding",
                            pty_id
                        );
                        Err(HelperErr::Timeout(String::new()))
                    }
                }
                VerifySubmitStrategy::None => Ok(()),
            };
            match res {
                Ok(()) => true,
                Err(HelperErr::PtyUnknown(id)) => {
                    return Err(InjectionError::PtyClosed { pty_id: id });
                }
                Err(HelperErr::Timeout(_)) => false,
            }
        } else {
            false
        };

        Ok(InjectionResult {
            bytes_written,
            echo_observed: false,
            submit_observed,
            elapsed: started.elapsed(),
        })
    }
}
