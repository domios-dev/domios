//! Verified injection path: send_shell_spawn, send_text_verified,
//! send_shell_spawn_verified.
use super::{build_spawn_dispatch, poll_shell_prompt};
use crate::service::ServiceContext;

impl ServiceContext {
    /// Launch a shell-level command in an attached pty (agent spawn, mem worker
    /// spawn, etc). Bypasses bracketed-paste — see `build_spawn_dispatch` for
    /// the rationale (bash 3.2 cannot interpret paste markers, the launch
    /// pipeline must arrive as a normal shell command line). Distinct from the
    /// interactive TUI input paths (`send_text_*`), which still use bracketed
    /// paste because Claude / Codex TUIs require it.
    pub fn send_shell_spawn(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        self.await_shell_ready(pty_id);
        let bytes = build_spawn_dispatch(cmd).map_err(|e| e.to_string())?;
        self.pty
            .lock()
            .write(pty_id, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Verified text injection. Holds NO ScreenState lock across the helper
    /// calls; callers must not hold mux / output_buf when invoking this.
    ///
    /// **Delivery is always best-effort: if `enter` is true, Enter (`\r`) is
    /// written to the PTY UNCONDITIONALLY after the bracket-paste payload.**
    /// Echo and submit verification are reporting fields on `InjectionResult`,
    /// not gates: a verify-timeout returns `Ok(... echo_observed: false ...)`
    /// rather than an error, and Enter still fires. This is the design fix for
    /// the long-standing bug where an echo-verify timeout (e.g. paste-placeholder
    /// matching, scroll-out, or TUIs that take >3s to render) caused Enter to
    /// be skipped entirely, leaving the recipient stuck in paste-mode forever.
    /// Hard errors are reserved for situations that genuinely prevent delivery
    /// (PTY closed, prompt never became ready when the caller required it).
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
        // #22: notify (never block) — an unsafe-prompt detection must not lock
        // the user out of sending; a stale/resolved menu used to hard-refuse here.
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

        // Empty text is a bare submit (e.g. `codeterm send --submit` to unstick a
        // paste already sitting in the composer): skip the bracketed paste so we
        // don't inject a stray space on Windows — just press Enter below.
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

        // Echo verify: informational only. Timeout means we couldn't confirm, not that the paste failed. PtyUnknown is the only real failure here and surfaces as PtyClosed.
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

        // #155: a TUI composer must finish ingesting the bracketed paste before
        // the submit Enter — a `\r` sent mid-ingest is swallowed as part of the
        // paste, leaving a long message typed-but-unsent. When we didn't already
        // wait on echo, block (bounded, suffix/placeholder-aware) until the paste
        // appears in the composer so the Enter lands as a real submit. Scoped to
        // TuiInputDrained providers (codex/claude); shell sends (ShellPrompt) and
        // bare submits (empty text) are unaffected.
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

        // Enter ALWAYS fires when the caller asked for it. This is the load-bearing line — no early-return above this point can skip it unless the PTY is already gone.
        let submit_observed = if enter {
            // Escalating Enter attempts: a `\r` is swallowed by a TUI still
            // ingesting the bracket paste (opencode). On a verify timeout we
            // re-send \r with a longer settle so the message actually submits.
            // Capped at 2 — more attempts multiply the per-attempt verify
            // timeout (codex's 15s drain check) into a minute-long stall.
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

    /// Shell-level agent launch with submit verification. Uses [`build_spawn_dispatch`]
    /// (temp script on Unix, bracketed paste on Windows) so bash 3.2 hosts never see
    /// raw ESC markers. Echo verification is skipped — the screen shows `bash /tmp/...`,
    /// not the original cmd.
    ///
    /// For `TuiInputDrained`: arms an event-driven `agent_signal` condvar that fires on
    /// the first `screen_has_agent_tui` hit in `ScreenState::process`. No timing race,
    /// no sleep: we block until actual TUI glyphs appear on screen.
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

        // Gate on shell-prompt readiness before writing — see `await_shell_ready`
        // (Windows ConPTY first-byte race → mangled launch like `c codex`).
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
