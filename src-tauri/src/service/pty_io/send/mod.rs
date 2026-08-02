//! Interactive send path: shell readiness and all
//! send/write/batch ServiceContext methods.
use crate::service::delivery_error::DeliveryError;
use crate::service::ServiceContext;

mod readiness;

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

pub(super) fn authenticated_pty_target(
    tab: &crate::multiplexer::types::Tab,
    expected_session_id: &str,
    expected_binding_nonce: &str,
) -> Result<(String, String), String> {
    let target = optimistic_agent_pty_target(tab)?;
    let authenticated = tab.agent_session_id.as_deref() == Some(expected_session_id)
        && tab.session_binding_nonce.as_deref() == Some(expected_binding_nonce)
        && tab.reported_session_binding_nonce.as_deref() == Some(expected_binding_nonce);
    if !authenticated {
        return Err("agent_unavailable: planner session is stale".into());
    }
    Ok(target)
}

pub(super) fn optimistic_agent_pty_target(
    tab: &crate::multiplexer::types::Tab,
) -> Result<(String, String), String> {
    // chat_view is UI mount state, not backend routability — a background or
    // hidden tab with a live agent PTY/session must still accept input.
    let routable = tab.agent_type.is_some();
    if !routable {
        return Err("agent_unavailable: recipient agent is not accepting input".into());
    }
    match &tab.backing {
        crate::multiplexer::types::TabBacking::Pty => Ok((
            tab.pty_id
                .clone()
                .ok_or_else(|| "agent_unavailable: recipient PTY missing".to_string())?,
            tab.agent_type.clone().unwrap_or_default(),
        )),
        crate::multiplexer::types::TabBacking::ChatBackend { .. } => {
            Err("agent_unavailable: recipient is not PTY-backed".into())
        }
    }
}

fn delivery_is_unknown(error: &str) -> bool {
    error.starts_with("submit_unconfirmed:")
}

fn public_agent_input_readiness(
    tab: &crate::multiplexer::types::Tab,
) -> Option<Result<(), String>> {
    if tab.agent_type.is_none() {
        return Some(Ok(()));
    }
    match tab.boot_stage {
        Some(codeterm_types::pty::BootStage::AgentReady)
            if !crate::service::boot_stage::blocking_prompt_owns_input(tab) =>
        {
            Some(Ok(()))
        }
        Some(codeterm_types::pty::BootStage::SpawnFailed) => Some(Err(tab
            .boot_failure_reason
            .clone()
            .unwrap_or_else(|| "agent launch failed".to_string()))),
        _ => None,
    }
}

#[derive(Clone)]
pub(crate) struct AuthenticatedPtyDelivery {
    pub(crate) pty_id: String,
    pub(crate) provider_id: String,
    pub(crate) permit: crate::pty::write_queue::GuardedInputPermit,
}

#[cfg(test)]
pub(super) fn submit_authenticated_pty_input<T>(
    tab: &crate::multiplexer::types::Tab,
    expected_session_id: &str,
    expected_binding_nonce: &str,
    text: &str,
    submit: impl FnOnce(&str, &str, &str) -> Result<T, String>,
) -> Result<T, String> {
    let (pty_id, provider_id) =
        authenticated_pty_target(tab, expected_session_id, expected_binding_nonce)?;
    submit(&pty_id, &provider_id, text)
}

pub(crate) fn sanitize_agent_prompt_input(text: &str) -> String {
    crate::pty::ansi::strip_ansi(text)
        .chars()
        .filter(|c| *c != '\0')
        .collect()
}

impl ServiceContext {
    fn authenticated_pty_delivery(
        &self,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
    ) -> Result<AuthenticatedPtyDelivery, String> {
        let (pty_id, provider_id) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| "agent_unavailable: planner Tab missing".to_string())?;
            authenticated_pty_target(tab, expected_session_id, expected_binding_nonce)?
        };
        let permit = self.pty.lock().issue_guarded_input_permit(&pty_id)?;
        Ok(AuthenticatedPtyDelivery {
            pty_id,
            provider_id,
            permit,
        })
    }

    pub(crate) fn send_to_agent_when_ready_detached(&self, tab_id: &str, text: &str) {
        let svc = self.clone();
        let tab = tab_id.to_string();
        let text = text.to_string();
        std::thread::spawn(move || {
            let pty_claim = svc.pty_ownership.acquire(&tab);
            let pty_id = {
                let mux = svc.mux.read();
                let target = mux
                    .get_tab(&tab)
                    .filter(|tab| tab.agent_type.is_some())
                    .ok_or_else(|| {
                        "agent_unavailable: recipient is no longer an agent".to_string()
                    });
                target.and_then(|tab| {
                    tab.pty_id
                        .clone()
                        .ok_or_else(|| "agent_unavailable: recipient PTY missing".to_string())
                })
            };
            let result = pty_id.and_then(|pty_id| svc.send_text_to_pty(&pty_id, &text));
            drop(pty_claim);
            if let Err(error) = result {
                if delivery_is_unknown(&error) {
                    log::warn!("[send-async] agent delivery to Tab {tab} remains unknown");
                    return;
                }
                log::warn!("[send-async] agent delivery to Tab {tab} failed: {error}");
                crate::api::events::emit_everywhere(
                    &svc.app_handle,
                    "chat-send-failed",
                    serde_json::json!({
                        "tab_id": &tab,
                        "error": &error,
                    }),
                );
            }
        });
    }

    pub(crate) fn send_to_authenticated_agent_session(
        &self,
        tab_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        text: &str,
    ) -> Result<(), String> {
        let delivery =
            self.authenticated_pty_delivery(tab_id, expected_session_id, expected_binding_nonce)?;
        self.send_authenticated_delivery_with_options(
            delivery,
            text,
            super::submit_transaction::PtySubmitEntry::Planner,
            true,
        )
    }

    fn send_authenticated_delivery_with_options(
        &self,
        delivery: AuthenticatedPtyDelivery,
        text: &str,
        entry: super::submit_transaction::PtySubmitEntry,
        verify_submit: bool,
    ) -> Result<(), String> {
        let strategy = self
            .agent_registry
            .with_provider(&delivery.provider_id, |provider| {
                provider.verify_submit_strategy()
            })
            .ok_or_else(|| "agent_unavailable: unknown planner provider".to_string())?;
        super::submit_transaction::submit_entry(entry, text, |text| {
            self.send_text_verified_guarded(
                &delivery.pty_id,
                text,
                true,
                strategy,
                delivery.permit.clone(),
                crate::service::inject_verify::VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: false,
                    verify_submit,
                    ..Default::default()
                },
            )
            .map(|_| ())
            .map_err(|error| {
                DeliveryError::from_injection(&delivery.pty_id, error).ledger_message()
            })
        })
    }

    /// Send text to a tab (chatBackend or PTY). Agent PTYs complete only after
    /// a causal turn transition or an actionable delivery failure.
    pub fn send_text(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.send_text_input_delivery(tab_id, text, true, None)
            .map_err(|error| error.ledger_message())
    }

    pub(crate) fn claim_chat_submit_cycle(
        &self,
        tab_id: &str,
    ) -> Option<super::submit_transaction::SubmitCycleClaim> {
        self.resolve_pty_id(tab_id)
            .ok()
            .map(|pty_id| super::submit_transaction::claim_submit_cycle(&pty_id))
    }

    pub(crate) fn send_chat_text_claimed(
        &self,
        tab_id: &str,
        text: &str,
        claim: Option<super::submit_transaction::SubmitCycleClaim>,
    ) -> Result<(), DeliveryError> {
        self.send_text_input_delivery(tab_id, text, true, claim)
    }

    pub(crate) fn insert_text(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.send_text_input_delivery(tab_id, text, false, None)
            .map_err(|error| error.ledger_message())
    }

    fn send_text_input_delivery(
        &self,
        tab_id: &str,
        text: &str,
        submit: bool,
        claim: Option<super::submit_transaction::SubmitCycleClaim>,
    ) -> Result<(), DeliveryError> {
        if self.tab_chat_backing(tab_id).is_some() {
            return self
                .chatbackend_tab_send(tab_id, text)
                .map_err(|error| DeliveryError::from_opaque(tab_id, error));
        }
        self.await_public_agent_input_ready_blocking(tab_id, std::time::Duration::from_secs(90))
            .map_err(|detail| DeliveryError::TabBusyRefused {
                tab: tab_id.to_string(),
                detail,
            })?;
        let pty_id = match self.resolve_pty_id(tab_id) {
            Ok(pty_id) => pty_id,
            Err(_) => self.prepare_tab_pty_delivery(tab_id)?,
        };
        let claim = claim
            .filter(|claim| claim.pty_id() == pty_id)
            .unwrap_or_else(|| super::submit_transaction::claim_submit_cycle(&pty_id));
        self.deliver_pty_input_claimed(&pty_id, text, submit, claim)
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

    pub fn send_text_to_pty(&self, pty_id: &str, text: &str) -> Result<(), String> {
        self.send_text_to_pty_input_delivery(pty_id, text, true)
            .map_err(|error| error.ledger_message())
    }

    pub(crate) fn insert_text_to_pty(&self, pty_id: &str, text: &str) -> Result<(), String> {
        self.send_text_to_pty_input_delivery(pty_id, text, false)
            .map_err(|error| error.ledger_message())
    }

    fn send_text_to_pty_input_delivery(
        &self,
        pty_id: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), DeliveryError> {
        let claim = super::submit_transaction::claim_submit_cycle(pty_id);
        self.deliver_pty_input_claimed(pty_id, text, submit, claim)
    }

    fn deliver_pty_input_claimed(
        &self,
        pty_id: &str,
        text: &str,
        submit: bool,
        claim: super::submit_transaction::SubmitCycleClaim,
    ) -> Result<(), DeliveryError> {
        let (tab_id, agent_type) = self.resolve_tab_and_agent(pty_id);
        let delivery_target = tab_id.as_deref().unwrap_or(pty_id);
        let text = agent_type
            .as_ref()
            .map(|_| sanitize_agent_prompt_input(text))
            .unwrap_or_else(|| text.to_string());
        let strategy = agent_type
            .as_deref()
            .and_then(|agent_id| {
                self.agent_registry
                    .with_provider(agent_id, |provider| provider.verify_submit_strategy())
            })
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
            claim,
        )
        .map(|_| ())
        .map_err(|error| DeliveryError::from_injection(delivery_target, error))
    }

    fn resolve_tab_and_agent(&self, pty_id: &str) -> (Option<String>, Option<String>) {
        let mux = self.mux.read();
        match mux.tab_id_for_pty(pty_id) {
            Some(pid) => (
                Some(pid.clone()),
                mux.get_tab(&pid).and_then(|p| p.agent_type.clone()),
            ),
            None => (None, None),
        }
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

    /// Queue text without making the caller wait for PTY or TUI evidence.
    pub async fn send_text_when_ready(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.send_text_detached(tab_id, text);
        Ok(())
    }

    pub(crate) async fn stage_text_when_ready(
        &self,
        tab_id: &str,
        text: &str,
    ) -> Result<(), String> {
        self.insert_text_detached(tab_id, text);
        Ok(())
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

    fn await_public_agent_input_ready_blocking(
        &self,
        tab_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let inspect = || {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Some(Err(format!("Tab {tab_id} no longer exists")));
            };
            public_agent_input_readiness(tab)
        };
        let mut observed = inspect();
        let completed =
            crate::service::boot_wait::wait_until(std::time::Instant::now() + timeout, || {
                observed = inspect();
                observed.is_some()
            });
        if completed {
            return observed.expect("completed public input readiness");
        }
        Err(format!(
            "Agent in Tab {tab_id} did not reach AgentReady within {timeout:?}"
        ))
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

    /// Write raw bytes (no bracket paste) — for keystrokes the TUI must interpret (vim j/k, arrows).
    pub fn write_raw(&self, tab_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(tab_id)?;
        self.pty
            .lock()
            .write(&pty_id, data)
            .map_err(|e| e.to_string())
    }

    /// Inject ordered raw key sequences off-thread so the state reactor never
    /// blocks on the pty lock. PTY-only — chatBackend tabs have no keys.
    pub fn inject_raw_keys_async(&self, tab_id: &str, writes: Vec<Vec<u8>>) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(tab_id)?;
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            for bytes in writes {
                let _ = pty.lock().write(&pty_id, &bytes);
            }
        });
        Ok(())
    }

    /// PTY work runs off-thread so CodeTerm-originated senders return immediately.
    /// Failures emit inline Chat status; delivery diagnostics never become
    /// global Notifications.
    pub fn send_text_detached(&self, tab_id: &str, text: &str) {
        self.deliver_text_detached(tab_id, text, true);
    }

    pub(crate) fn insert_text_detached(&self, tab_id: &str, text: &str) {
        self.deliver_text_detached(tab_id, text, false);
    }

    fn deliver_text_detached(&self, tab_id: &str, text: &str, submit: bool) {
        let svc = self.clone();
        let tab = tab_id.to_string();
        let text = text.to_string();
        std::thread::spawn(move || {
            let result = if submit {
                svc.send_text(&tab, &text)
            } else {
                svc.insert_text(&tab, &text)
            };
            if let Err(error) = result {
                if delivery_is_unknown(&error) {
                    log::warn!("[send-async] delivery to Tab {tab} remains unknown");
                    return;
                }
                log::warn!("[send-async] delivery to Tab {tab} failed: {error}");
                crate::api::events::emit_everywhere(
                    &svc.app_handle,
                    "chat-send-failed",
                    serde_json::json!({
                        "tab_id": &tab,
                        "error": &error,
                    }),
                );
            }
        });
    }

    /// Feed payloads to the typed-launch input parser so the CLI/API path triggers
    /// the same `agent_type` lock as a direct Tauri write. Skips locked tabs.
    pub(crate) fn feed_input_parser(&self, pty_id: &str, text: &str, enter: bool) {
        let (tab_id, locked) = {
            let m = self.mux.read();
            let pid = m.tab_id_for_pty(pty_id);
            let locked = pid
                .as_ref()
                .and_then(|id| m.get_tab(id))
                .map(|p| p.agent_type_locked)
                .unwrap_or(false);
            (pid, locked)
        };
        let Some(tab_id) = tab_id else { return };
        if locked {
            self.input_parser.clear(&tab_id);
            return;
        }
        let mut payload = text.as_bytes().to_vec();
        if enter {
            payload.push(b'\r');
        }
        if let Some(provider_id) = self.input_parser.process(&tab_id, &payload) {
            log::info!(
                "[input-parser] detected '{}' send in Tab {} — locking agent_type",
                provider_id,
                tab_id,
            );
            self.set_and_lock_tab_agent_type(&tab_id, provider_id);
            self.emit_state_changed();
        }
    }
}

#[cfg(test)]
mod tests;
