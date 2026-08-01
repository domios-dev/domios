//! Interactive send path: shell readiness and all
//! send/write/batch ServiceContext methods.
use crate::service::delivery_error::DeliveryError;
use crate::service::ServiceContext;

mod authenticated;
mod ingress;
mod readiness;

use authenticated::optimistic_agent_pty_target;
#[cfg(test)]
use authenticated::{authenticated_pty_target, submit_authenticated_pty_input};

pub(crate) use readiness::provider_or_generic_tui;
#[cfg(test)]
use readiness::{provider_frame_ready, wait_for_observed_readiness};

/// Bounded window to let the composer render a bracketed paste before Enter is
/// sent as a separate write. A codex/opencode TUI swallows a \r glued to the
/// paste mid-ingestion; waiting for the paste to land avoids that. The deadline
/// is a failure bound only — it returns early on the render signal, and a missed
/// deadline still submits (optimistic, never a drop).
/// Bytes written to a fresh pty to launch a shell-level command. On Unix, exec a
/// temp script via `/bin/bash <path>` to bypass bracketed-paste markers — macOS system
/// bash 3.2 has no bracketed-paste binding and leaks the markers as literal text.
/// Script self-cleans via `trap 'rm -f $0' EXIT`.
#[cfg(unix)]
pub(crate) fn build_spawn_dispatch(cmd: &str) -> std::io::Result<Vec<u8>> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let script_path = format!("/tmp/codeterm-spawn-{}.sh", uuid::Uuid::new_v4());
    {
        let mut f = std::fs::File::create(&script_path)?;
        writeln!(f, "#!/usr/bin/env bash")?;
        writeln!(f, "trap 'rm -f \"$0\"' EXIT")?;
        writeln!(f, "{}", cmd)?;
    }
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))?;
    Ok(format!("/bin/bash {}\r", script_path).into_bytes())
}

/// Windows keeps bracketed paste (no /bin/bash to script-exec). The leading space
/// is sacrificial: it absorbs PSReadLine's dropped first byte before the paste handler arms.
#[cfg(windows)]
pub(crate) fn build_spawn_dispatch(cmd: &str) -> std::io::Result<Vec<u8>> {
    Ok(format!("\x1b[200~ {}\x1b[201~", cmd).into_bytes())
}

/// Re-arms allowed after the armed signal is torn down by a generation rebind.
/// Each one needs a real teardown event, so this only bounds a pathological loop.
const SHELL_READY_REARM_LIMIT: u32 = 4;

/// Wait (event-driven, via a condvar armed under `screen_state`) for a shell
/// prompt to appear on a PTY. Returns `true` if ready, `false` on timeout.
///
/// A signal closed by the reader binding its generation is NOT a deadline: the
/// wait re-arms and keeps the original deadline, instead of reporting a false
/// immediate timeout and leaving the relaunch to a watchdog tick.
pub fn await_shell_prompt(
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    await_shell_prompt_armed(screen_state, pty_id, timeout, |_| {})
}

/// `on_armed` fires with the re-arm count each time the wait is armed, before
/// it blocks — the ordering hook the arm-vs-bind regression test needs.
pub(super) fn await_shell_prompt_armed(
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    timeout: std::time::Duration,
    mut on_armed: impl FnMut(u32),
) -> bool {
    let short_id = &pty_id[..pty_id.len().min(8)];
    let t0 = std::time::Instant::now();
    let mut rearms = 0u32;

    loop {
        let (signal, bound) = {
            let mut ss = screen_state.lock();
            (
                ss.arm_shell_ready_signal(pty_id),
                ss.has_pty_generation(pty_id),
            )
        };
        if !bound {
            log::debug!(
                "[await_shell_prompt] armed pty {} before its reader bound a generation",
                short_id
            );
        }
        let Some(remaining) = timeout.checked_sub(t0.elapsed()) else {
            break;
        };
        on_armed(rearms);
        if signal.wait(remaining) {
            log::info!(
                "[await_shell_prompt] shell ready for pty {} after {}ms",
                short_id,
                t0.elapsed().as_millis()
            );
            return true;
        }
        if !signal.is_closed() || rearms >= SHELL_READY_REARM_LIMIT {
            break;
        }
        rearms += 1;
        log::info!(
            "[await_shell_prompt] pty {} readiness signal closed after {}ms (rebind {}); re-arming",
            short_id,
            t0.elapsed().as_millis(),
            rearms
        );
    }

    log::warn!(
        "[await_shell_prompt] pty {} not ready after {}ms (deadline {}ms, {} re-arms)",
        short_id,
        t0.elapsed().as_millis(),
        timeout.as_millis(),
        rearms
    );
    false
}

/// Wait for a real shell prompt, rejecting agent TUI composers that also use
/// prompt glyphs. Use before writing launch/resume commands into a restored PTY.
pub fn await_bare_shell_prompt(
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let signal = screen_state.lock().arm_bare_prompt_signal(pty_id);
    let short_id = &pty_id[..pty_id.len().min(8)];
    let t0 = std::time::Instant::now();
    let ready = signal.wait(timeout);
    screen_state
        .lock()
        .disarm_bare_prompt_signal(pty_id, &signal);
    if ready {
        log::info!(
            "[await_bare_shell_prompt] shell ready for pty {} after {}ms",
            short_id,
            t0.elapsed().as_millis()
        );
    } else {
        log::warn!(
            "[await_bare_shell_prompt] pty {} not ready after {}ms (deadline {}ms)",
            short_id,
            t0.elapsed().as_millis(),
            timeout.as_millis()
        );
    }
    ready
}

fn delivery_is_unknown(error: &str) -> bool {
    error.starts_with("submit_unconfirmed:")
}

pub(crate) fn sanitize_agent_prompt_input(text: &str) -> String {
    crate::pty::ansi::strip_ansi(text)
        .chars()
        .filter(|c| *c != '\0')
        .collect()
}

impl ServiceContext {
    /// Send text to a tab (chatBackend or PTY). Agent PTYs complete only after
    /// a causal turn transition or an actionable delivery failure.
    pub fn send_text(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.send_text_delivery(tab_id, text)
            .map_err(|error| error.ledger_message())
    }

    pub(crate) fn send_text_delivery(&self, tab_id: &str, text: &str) -> Result<(), DeliveryError> {
        self.send_text_input_delivery(tab_id, text, true)
    }

    pub(crate) fn send_text_input(
        &self,
        tab_id: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), String> {
        self.send_text_input_delivery(tab_id, text, submit)
            .map_err(|error| error.ledger_message())
    }

    fn send_text_input_delivery(
        &self,
        tab_id: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), DeliveryError> {
        let submit_claim = self
            .resolve_pty_id(tab_id)
            .ok()
            .map(|pty_id| super::submit_transaction::claim_submit_cycle(&pty_id));
        self.send_text_input_delivery_claimed(tab_id, text, submit, submit_claim)
    }

    fn send_text_input_delivery_claimed(
        &self,
        tab_id: &str,
        text: &str,
        submit: bool,
        submit_claim: Option<super::submit_transaction::SubmitCycleClaim>,
    ) -> Result<(), DeliveryError> {
        if self.tab_chat_backing(tab_id).is_some() {
            return self
                .chatbackend_tab_send(tab_id, text)
                .map_err(|error| DeliveryError::from_opaque(tab_id, error));
        }
        let pty_id = self.prepare_tab_pty_delivery(tab_id)?;
        let submit_claim = match submit_claim {
            Some(claim) if claim.pty_id() == pty_id => claim,
            Some(claim) => {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.to_string(),
                    detail: format!(
                        "PTY changed after input ingress: {} -> {}",
                        claim.pty_id(),
                        pty_id
                    ),
                })
            }
            None => super::submit_transaction::claim_submit_cycle(&pty_id),
        };
        self.send_text_to_pty_smart_delivery_claimed(&pty_id, text, submit, submit_claim)
    }

    fn prepare_tab_pty_delivery(&self, tab_id: &str) -> Result<String, DeliveryError> {
        self.ensure_tab_pty_for_send(tab_id)
            .map_err(|detail| DeliveryError::PtyUnavailable {
                tab: tab_id.to_string(),
                detail,
            })?;
        self.await_agent_boot_blocking(tab_id, std::time::Duration::from_secs(90))
            .map_err(|detail| DeliveryError::TabBusyRefused {
                tab: tab_id.to_string(),
                detail,
            })?;
        let pty_id = self
            .await_pty_link_blocking(tab_id, std::time::Duration::from_secs(15))
            .map_err(|detail| DeliveryError::PtyUnavailable {
                tab: tab_id.to_string(),
                detail,
            })?;
        self.await_tab_routable_blocking(tab_id, std::time::Duration::from_secs(30))
            .map_err(|detail| DeliveryError::TabBusyRefused {
                tab: tab_id.to_string(),
                detail,
            })?;
        Ok(pty_id)
    }

    /// Compatibility entry point for callers which explicitly require delivery
    /// confirmation. `send_text` now applies the same transaction contract.
    pub fn send_text_with_guarantee(&self, tab_id: &str, text: &str) -> Result<(), DeliveryError> {
        if self.tab_chat_backing(tab_id).is_some() {
            return self
                .chatbackend_tab_send(tab_id, text)
                .map_err(|error| DeliveryError::from_opaque(tab_id, error));
        }
        let submit_claim = self
            .resolve_pty_id(tab_id)
            .ok()
            .map(|pty_id| super::submit_transaction::claim_submit_cycle(&pty_id));
        let pty_id = self.prepare_tab_pty_delivery(tab_id)?;
        let submit_claim = match submit_claim {
            Some(claim) if claim.pty_id() == pty_id => claim,
            Some(claim) => {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.to_string(),
                    detail: format!(
                        "PTY changed after input ingress: {} -> {}",
                        claim.pty_id(),
                        pty_id
                    ),
                })
            }
            None => super::submit_transaction::claim_submit_cycle(&pty_id),
        };
        self.send_text_to_pty_smart_guaranteed_claimed(&pty_id, text, submit_claim)
    }

    /// Capability-aware delivery. PTY agent transactions remain pending until
    /// a causal turn transition or an actionable failure.
    pub fn send_text_to_pty_smart(&self, pty_id: &str, text: &str) -> Result<(), String> {
        self.send_text_to_pty_smart_delivery(pty_id, text, true)
            .map_err(|error| error.ledger_message())
    }

    pub(crate) fn send_text_to_pty_input(
        &self,
        pty_id: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), String> {
        self.send_text_to_pty_smart_delivery(pty_id, text, submit)
            .map_err(|error| error.ledger_message())
    }

    fn send_text_to_pty_smart_delivery(
        &self,
        pty_id: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), DeliveryError> {
        let submit_claim = super::submit_transaction::claim_submit_cycle(pty_id);
        self.send_text_to_pty_smart_delivery_claimed(pty_id, text, submit, submit_claim)
    }

    fn send_text_to_pty_smart_delivery_claimed(
        &self,
        pty_id: &str,
        text: &str,
        submit: bool,
        submit_claim: super::submit_transaction::SubmitCycleClaim,
    ) -> Result<(), DeliveryError> {
        let (tab_id, agent_type) = self.resolve_tab_and_ready_agent(pty_id);
        if let Some(ref pid) = tab_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))
                .map_err(|detail| DeliveryError::TabBusyRefused {
                    tab: pid.clone(),
                    detail,
                })?;
        }
        let delivery_target = tab_id.as_deref().unwrap_or(pty_id);
        let Some(agent_id) = agent_type else {
            return self.send_text_to_pty_delivery_claimed(
                pty_id,
                delivery_target,
                text,
                submit,
                submit_claim,
            );
        };
        let text = sanitize_agent_prompt_input(text);
        let strategy = self
            .agent_registry
            .with_provider(&agent_id, |provider| provider.verify_submit_strategy())
            .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
        let submit_key: &[u8] = b"\r";
        super::submit_transaction::submit_text_with_key_claimed(
            self,
            pty_id,
            &text,
            submit,
            submit_key,
            strategy,
            crate::service::inject_verify::VerifyOpts {
                require_prompt_first: false,
                verify_echo: true,
                verify_submit: submit,
                ..Default::default()
            },
            None,
            submit_claim,
        )
        .map(|_| ())
        .map_err(|error| DeliveryError::from_injection(delivery_target, error))
    }

    fn resolve_tab_and_ready_agent(&self, pty_id: &str) -> (Option<String>, Option<String>) {
        let mux = self.mux.read();
        match mux.tab_id_for_pty(pty_id) {
            Some(pid) => {
                let at = mux.get_tab(&pid).and_then(|p| {
                    (p.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady))
                        .then(|| p.agent_type.clone())
                        .flatten()
                });
                (Some(pid), at)
            }
            None => (None, None),
        }
    }

    /// Verified twin of `send_text_to_pty_smart` — waits for echo/submit
    /// evidence. Parked: terminal delivery can't be reliably confirmed (TUIs
    /// intercept input, ConPTY echo is not proof), so production uses the
    /// optimistic path. Kept for callers that can tolerate its false negatives.
    pub fn send_text_to_pty_smart_guaranteed(
        &self,
        pty_id: &str,
        text: &str,
    ) -> Result<(), DeliveryError> {
        let submit_claim = super::submit_transaction::claim_submit_cycle(pty_id);
        self.send_text_to_pty_smart_guaranteed_claimed(pty_id, text, submit_claim)
    }

    fn send_text_to_pty_smart_guaranteed_claimed(
        &self,
        pty_id: &str,
        text: &str,
        submit_claim: super::submit_transaction::SubmitCycleClaim,
    ) -> Result<(), DeliveryError> {
        let (tab_id, agent_type) = self.resolve_tab_and_ready_agent(pty_id);
        if let Some(ref pid) = tab_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))
                .map_err(|detail| DeliveryError::TabBusyRefused {
                    tab: pid.clone(),
                    detail,
                })?;
        }
        let delivery_target = tab_id.as_deref().unwrap_or(pty_id);
        if let Some(ref agent_id) = agent_type {
            let text = sanitize_agent_prompt_input(text);
            let strategy = self
                .agent_registry
                .with_provider(agent_id, |p| p.verify_submit_strategy())
                .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
            let opts = crate::service::inject_verify::VerifyOpts {
                // A live TUI's last line is its footer, not a shell prompt.
                require_prompt_first: false,
                verify_echo: true,
                verify_submit: true,
                ..Default::default()
            };
            return super::submit_transaction::submit_entry(
                super::submit_transaction::PtySubmitEntry::Direct,
                &text,
                |text| {
                    self.send_text_verified_claimed(
                        pty_id,
                        text,
                        true,
                        strategy,
                        opts,
                        submit_claim,
                    )
                    .map(|_| ())
                    .map_err(|error| DeliveryError::from_injection(delivery_target, error))
                },
            );
        }
        self.send_text_to_pty_delivery_claimed(pty_id, delivery_target, text, true, submit_claim)
    }

    /// Deliver the initial agent launch command to a bare shell — no verify path
    /// (no agent TUI exists yet). Mirrors general_agent::lifecycle.
    pub fn send_launch_command_to_pty(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::time::Duration;
            if !await_shell_prompt(&self.screen_state, pty_id, Duration::from_secs(4)) {
                let _ = self.pty.lock().write(pty_id, b"\x1b[1;1R");
                await_shell_prompt(&self.screen_state, pty_id, Duration::from_secs(4));
            }
        }
        let payload: Vec<u8> = if cfg!(windows) {
            format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes()
        } else {
            format!("{}\r", cmd).into_bytes()
        };
        self.pty.lock().write(pty_id, &payload)
    }

    pub(crate) fn await_agent_boot_blocking(
        &self,
        tab_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let ready =
            crate::service::boot_wait::wait_until(std::time::Instant::now() + timeout, || {
                !self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .map(|tab| tab.agent_booting)
                    .unwrap_or(false)
            });
        if ready {
            Ok(())
        } else {
            Err(format!(
                "Agent in Tab {} is still booting — input would be lost",
                tab_id
            ))
        }
    }

    /// Windows ConPTY: a write before the prompt renders races the first byte and
    /// mangles the command, so block until the prompt arms PSReadLine. No-op on Unix.
    pub(crate) fn await_shell_ready(&self, pty_id: &str) {
        #[cfg(windows)]
        await_shell_prompt(
            &self.screen_state,
            pty_id,
            std::time::Duration::from_secs(10),
        );
        #[cfg(not(windows))]
        let _ = pty_id;
    }

    pub(crate) fn await_pty_link_blocking(
        &self,
        tab_id: &str,
        timeout: std::time::Duration,
    ) -> Result<String, String> {
        crate::service::orchestrator::tab_relink_signal::wait_for_relink(
            std::time::Instant::now() + timeout,
            || self.resolve_pty_id(tab_id).ok(),
        )
        .ok_or_else(|| {
            format!(
                "Tab {} not found or PTY did not start within {:?}",
                tab_id, timeout
            )
        })
    }

    /// Wait until the tab is routable for chat delivery. For non-agent tabs (bare shells) this
    /// returns immediately. The wait uses the boot_wait condvar so it resolves the
    /// instant the tab state transitions — no polling. Does NOT gate on
    /// `chat_view` — that is UI mount state, not backend routability. A
    /// background/hidden/restored tab is routable as soon as its PTY is live.
    pub(crate) fn await_tab_routable_blocking(
        &self,
        tab_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let needs_routability = {
            let mux = self.mux.read();
            match mux.get_tab(tab_id) {
                Some(tab) => tab.agent_type.is_some(),
                None => return Ok(()),
            }
        };
        if !needs_routability {
            return Ok(());
        }
        let ready =
            crate::service::boot_wait::wait_until(std::time::Instant::now() + timeout, || {
                self.mux
                    .read()
                    .get_tab(tab_id)
                    .is_some_and(|tab| optimistic_agent_pty_target(tab).is_ok())
            });
        if ready {
            Ok(())
        } else {
            Err(format!(
                "Agent in Tab {} did not become routable within {:?}",
                tab_id, timeout
            ))
        }
    }

    /// `send_text` by PTY id, skipping tab→pty resolution — use when the caller
    /// holds a verified pty_id and must not race a `link_pty` swap of `tab.pty_id`.
    pub fn send_text_to_pty(&self, pty_id: &str, text: &str) -> Result<(), String> {
        let tab_id = self.resolve_tab_and_ready_agent(pty_id).0;
        self.send_text_to_pty_delivery(pty_id, tab_id.as_deref().unwrap_or(pty_id), text, true)
            .map_err(|error| error.ledger_message())
    }

    fn send_text_to_pty_delivery(
        &self,
        pty_id: &str,
        delivery_target: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), DeliveryError> {
        let submit_claim = super::submit_transaction::claim_submit_cycle(pty_id);
        self.send_text_to_pty_delivery_claimed(pty_id, delivery_target, text, enter, submit_claim)
    }

    fn send_text_to_pty_delivery_claimed(
        &self,
        pty_id: &str,
        delivery_target: &str,
        text: &str,
        enter: bool,
        submit_claim: super::submit_transaction::SubmitCycleClaim,
    ) -> Result<(), DeliveryError> {
        crate::service::inject_safety::notify_if_unsafe_prompt(&self.screen_state, pty_id);
        super::submit_transaction::submit_entry(
            super::submit_transaction::PtySubmitEntry::Direct,
            text,
            |text| {
                super::submit_transaction::submit_text_with_key_claimed(
                    self,
                    pty_id,
                    text,
                    enter,
                    b"\r",
                    crate::agent::provider::VerifySubmitStrategy::None,
                    crate::service::inject_verify::VerifyOpts {
                        require_prompt_first: false,
                        verify_echo: enter,
                        verify_submit: enter,
                        ..Default::default()
                    },
                    None,
                    submit_claim,
                )
                .map(|_| ())
                .map_err(|error| DeliveryError::from_injection(delivery_target, error))
            },
        )
    }

    /// Launch-time delivery targets the PTY already owned by the lifecycle.
    /// It must not enter the public post-ready Tab send gate.
    pub(crate) fn send_launch_text_optimistic(
        &self,
        pty_id: &str,
        provider_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<
        crate::service::inject_verify::InjectionResult,
        crate::service::inject_verify::InjectionError,
    > {
        let strategy = self
            .agent_registry
            .with_provider(provider_id, |provider| provider.verify_submit_strategy())
            .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
        super::submit_transaction::submit_entry(
            super::submit_transaction::PtySubmitEntry::Direct,
            &text,
            |text| {
                super::submit_transaction::submit_text(
                    self,
                    pty_id,
                    text,
                    enter,
                    strategy,
                    crate::service::inject_verify::VerifyOpts {
                        require_prompt_first: false,
                        verify_echo: true,
                        verify_submit: enter,
                        ..Default::default()
                    },
                )
            },
        )
    }
}

#[cfg(test)]
mod tests;
