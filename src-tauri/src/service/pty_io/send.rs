//! Interactive send path: shell readiness and all
//! send/write/batch ServiceContext methods.
use crate::service::ServiceContext;

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

/// Wait (event-driven, via a condvar armed under `screen_state`) for a shell
/// prompt to appear on a PTY. Returns `true` if ready, `false` on timeout.
pub fn await_shell_prompt(
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let signal = screen_state.lock().arm_shell_ready_signal(pty_id);
    let short_id = &pty_id[..pty_id.len().min(8)];
    let t0 = std::time::Instant::now();
    let ready = signal.wait(timeout);
    if ready {
        log::info!(
            "[await_shell_prompt] shell ready for pty {} after {}ms",
            short_id,
            t0.elapsed().as_millis()
        );
    } else {
        log::warn!(
            "[await_shell_prompt] timeout after {}ms for pty {}",
            timeout.as_millis(),
            short_id
        );
    }
    ready
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
            "[await_bare_shell_prompt] timeout after {}ms for pty {}",
            timeout.as_millis(),
            short_id
        );
    }
    ready
}

fn authenticated_pty_target(
    pane: &crate::multiplexer::types::Pane,
    expected_session_id: &str,
    expected_binding_nonce: &str,
) -> Result<(String, String), String> {
    let accepting = match pane.agent_state.as_ref() {
        Some(state) => matches!(
            state.state,
            codeterm_types::pty::TimelineState::Working | codeterm_types::pty::TimelineState::Idle
        ),
        None => pane.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady),
    };
    let authenticated = pane.agent_type.is_some()
        && pane.agent_session_id.as_deref() == Some(expected_session_id)
        && pane.session_binding_nonce.as_deref() == Some(expected_binding_nonce)
        && pane.reported_session_binding_nonce.as_deref() == Some(expected_binding_nonce)
        && pane.chat_view
        && accepting;
    if !authenticated {
        return Err("agent_unavailable: planner session is stale".into());
    }
    match &pane.backing {
        crate::multiplexer::types::PaneBacking::Pty => Ok((
            pane.pty_id
                .clone()
                .ok_or_else(|| "agent_unavailable: planner PTY missing".to_string())?,
            pane.agent_type.clone().unwrap_or_default(),
        )),
        crate::multiplexer::types::PaneBacking::ChatBackend { .. } => {
            Err("agent_unavailable: planner is not PTY-backed".into())
        }
    }
}

fn submit_authenticated_pty_input<T>(
    pane: &crate::multiplexer::types::Pane,
    expected_session_id: &str,
    expected_binding_nonce: &str,
    text: &str,
    submit: impl FnOnce(&str, &str, &str) -> Result<T, String>,
) -> Result<T, String> {
    let (pty_id, provider_id) =
        authenticated_pty_target(pane, expected_session_id, expected_binding_nonce)?;
    submit(&pty_id, &provider_id, text)
}

pub(crate) fn sanitize_agent_prompt_input(text: &str) -> String {
    crate::pty::ansi::strip_ansi(text)
        .chars()
        .filter(|c| *c != '\0')
        .collect()
}

impl ServiceContext {
    pub(crate) fn send_to_authenticated_agent_session(
        &self,
        pane_id: &str,
        expected_session_id: &str,
        expected_binding_nonce: &str,
        text: &str,
    ) -> Result<(), String> {
        let target = || -> Result<(String, String), String> {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_pane_id(pane_id)
                .ok_or_else(|| "agent_unavailable: planner pane missing".to_string())?;
            let pane = mux
                .get_pane(&resolved)
                .ok_or_else(|| "agent_unavailable: planner pane missing".to_string())?;
            submit_authenticated_pty_input(
                pane,
                expected_session_id,
                expected_binding_nonce,
                text,
                |pty_id, provider_id, _| Ok((pty_id.to_string(), provider_id.to_string())),
            )
        };

        let (pty_id, provider_id) = target()?;
        let strategy = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.verify_submit_strategy())
            .ok_or_else(|| "agent_unavailable: unknown planner provider".to_string())?;
        if target()? != (pty_id.clone(), provider_id.clone()) {
            return Err("agent_unavailable: planner transport changed".into());
        }
        super::submit_transaction::submit_entry(
            super::submit_transaction::PtySubmitEntry::Planner,
            text,
            |text| {
                self.send_text_verified(
                    &pty_id,
                    text,
                    true,
                    strategy,
                    crate::service::inject_verify::VerifyOpts {
                        require_prompt_first: false,
                        verify_echo: false,
                        verify_submit: true,
                        ..Default::default()
                    },
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            },
        )
    }

    /// Send text to a pane (chatBackend or PTY). Agent PTYs use causal paste
    /// ingestion and bounded recovery, but acknowledgement misses stay diagnostic.
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        if self.pane_chat_backing(pane_id).is_some() {
            return self.chatbackend_pane_send(pane_id, text);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;

        self.send_text_to_pty_smart(&pty_id, text, enter)
    }

    /// Verified twin of `send_text` — waits for echo/submit evidence. Parked:
    /// terminal delivery can't be reliably confirmed, so production uses the
    /// optimistic `send_text`. Not wired to any caller yet.
    pub fn send_text_with_guarantee(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        if self.pane_chat_backing(pane_id).is_some() {
            return self.chatbackend_pane_send(pane_id, text);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;

        self.send_text_to_pty_smart_guaranteed(&pty_id, text, enter)
    }

    /// Provider-aware delivery. PTY agents use causal paste ingestion and
    /// bounded recovery; only zero-byte transport failures reject the send.
    pub fn send_text_to_pty_smart(
        &self,
        pty_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        let (pane_id, agent_type) = self.resolve_pane_and_agent(pty_id);
        if let Some(ref pid) = pane_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))?;
        }
        let Some(agent_id) = agent_type else {
            return self.write_paste_optimistic(pty_id, text, enter);
        };
        let text = sanitize_agent_prompt_input(text);
        let (strategy, busy_submit) = self
            .agent_registry
            .with_provider(&agent_id, |provider| {
                (
                    provider.verify_submit_strategy(),
                    provider.busy_submit_strategy(),
                )
            })
            .unwrap_or((
                crate::agent::provider::VerifySubmitStrategy::None,
                crate::agent::provider::BusySubmitStrategy::SubmitNormally,
            ));
        let is_working = matches!(
            self.agent_tracker.lock().get_status_if_tracked(pty_id),
            Some(crate::agent::status::PaneStatus::Working)
        );
        let submit_key = if enter && is_working {
            match busy_submit {
                crate::agent::provider::BusySubmitStrategy::Key(key) => {
                    crate::agent::keys::action_key_bytes(&key).ok_or_else(|| {
                        format!("provider declared unknown busy submit key: {key}")
                    })?
                }
                crate::agent::provider::BusySubmitStrategy::SubmitNormally => b"\r".to_vec(),
            }
        } else {
            b"\r".to_vec()
        };
        super::submit_transaction::submit_text_with_key(
            self,
            pty_id,
            &text,
            enter,
            &submit_key,
            strategy,
            crate::service::inject_verify::VerifyOpts {
                require_prompt_first: false,
                verify_echo: true,
                verify_submit: false,
                ..Default::default()
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    /// Bracketed paste + optional Enter, written straight to the PTY with no
    /// verification wait. The agent-type input parser still sees the payload.
    fn write_paste_optimistic(&self, pty_id: &str, text: &str, enter: bool) -> Result<(), String> {
        self.feed_input_parser(pty_id, text, enter);
        let paste = if cfg!(windows) {
            format!("\x1b[200~ {}\x1b[201~", text)
        } else {
            format!("\x1b[200~{}\x1b[201~", text)
        };
        if !enter {
            return self
                .pty
                .lock()
                .write(pty_id, paste.as_bytes())
                .map_err(|e| e.to_string());
        }
        // Enter follows the paste-render event because TUIs can swallow a glued CR.
        // The bounded deadline still submits optimistically; it never proves readiness.
        let signal = self.screen_state.lock().arm_screen_change_signal(pty_id);
        let generation = signal.generation();
        self.pty
            .lock()
            .write(pty_id, paste.as_bytes())
            .map_err(|e| e.to_string())?;
        signal.wait_after(generation, super::PASTE_RENDER_WINDOW);
        self.pty
            .lock()
            .write(pty_id, b"\r")
            .map_err(|e| e.to_string())
    }

    fn resolve_pane_and_agent(&self, pty_id: &str) -> (Option<String>, Option<String>) {
        let mux = self.mux.read();
        match mux.pane_id_for_pty(pty_id) {
            Some(pid) => {
                let at = mux.get_pane(&pid).and_then(|p| p.agent_type.clone());
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
        enter: bool,
    ) -> Result<(), String> {
        let (pane_id, agent_type) = self.resolve_pane_and_agent(pty_id);
        if let Some(ref pid) = pane_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))?;
        }
        if let Some(ref agent_id) = agent_type {
            let text = sanitize_agent_prompt_input(text);
            let strategy = self
                .agent_registry
                .with_provider(agent_id, |p| p.verify_submit_strategy())
                .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
            let opts = crate::service::inject_verify::VerifyOpts {
                // A live TUI's last line is its footer, not a shell prompt.
                require_prompt_first: false,
                verify_echo: false,
                verify_submit: true,
                ..Default::default()
            };
            return super::submit_transaction::submit_entry(
                super::submit_transaction::PtySubmitEntry::Direct,
                &text,
                |text| {
                    self.send_text_verified(pty_id, text, enter, strategy, opts)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
        }
        self.send_text_to_pty(pty_id, text, enter)
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

    /// Wait until an agent has rendered a structural TUI frame. This proves a
    /// shell launch command ran; it does not claim the input composer is ready.
    pub(crate) fn await_agent_tui_frame_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> bool {
        self.await_agent_tui_screen(pty_id, agent_id, timeout, false)
    }

    /// Wait until a structural agent TUI exposes an operable composer.
    pub(crate) fn await_agent_tui_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> bool {
        self.await_agent_tui_screen(pty_id, agent_id, timeout, true)
    }

    fn await_agent_tui_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
        require_composer: bool,
    ) -> bool {
        let signal = self.screen_state.lock().arm_screen_change_signal(pty_id);
        let deadline = std::time::Instant::now() + timeout;
        let mut generation = signal.generation();
        loop {
            let (screen, composer_ready) = {
                let screen_state = self.screen_state.lock();
                let screen = screen_state.get_screen_text(pty_id).unwrap_or_default();
                let cursor = screen_state.cursor_input_state(pty_id);
                let composer_ready = cursor
                    .map(|(_alt, visible, row, rows)| {
                        crate::agent::offline_state_classifier::composer_cursor_ready(
                            visible, row, rows,
                        )
                    })
                    .unwrap_or(false);
                (screen, composer_ready)
            };
            let provider_ready = self
                .agent_registry
                .with_provider(agent_id, |provider| provider.screen_has_tui(&screen));
            let provider_prompt = self
                .agent_registry
                .parse_prompt(agent_id, &screen)
                .is_some();
            let ready = provider_or_generic_tui(
                provider_ready,
                codeterm_pty::pty::screen_state::screen_has_agent_tui(&screen),
            );
            if provider_frame_ready(ready, provider_prompt, require_composer, composer_ready) {
                return true;
            }
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return false;
            };
            if !signal.wait_after(generation, remaining) {
                return false;
            }
            generation = signal.generation();
        }
    }

    /// Queue text without making the caller wait for PTY or TUI evidence.
    pub async fn send_text_when_ready(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        self.send_text_detached(pane_id, text, enter);
        Ok(())
    }

    pub(crate) fn await_agent_boot_blocking(
        &self,
        pane_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let ready =
            crate::service::boot_wait::wait_until(std::time::Instant::now() + timeout, || {
                !self
                    .mux
                    .read()
                    .get_pane(pane_id)
                    .map(|pane| pane.agent_booting)
                    .unwrap_or(false)
            });
        if ready {
            Ok(())
        } else {
            Err(format!(
                "Agent in pane {} is still booting — input would be lost",
                pane_id
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
        pane_id: &str,
        timeout: std::time::Duration,
    ) -> Result<String, String> {
        crate::service::orchestrator::pane_relink_signal::wait_for_relink(
            std::time::Instant::now() + timeout,
            || self.resolve_pty_id(pane_id).ok(),
        )
        .ok_or_else(|| {
            format!(
                "Pane {} not found or PTY did not start within {:?}",
                pane_id, timeout
            )
        })
    }

    /// `send_text` by PTY id, skipping pane→pty resolution — use when the caller
    /// holds a verified pty_id and must not race a `link_pty` swap of `pane.pty_id`.
    pub fn send_text_to_pty(&self, pty_id: &str, text: &str, enter: bool) -> Result<(), String> {
        super::submit_transaction::submit_entry(
            super::submit_transaction::PtySubmitEntry::Direct,
            text,
            |text| {
                self.send_text_verified(
                    pty_id,
                    text,
                    enter,
                    crate::agent::provider::VerifySubmitStrategy::None,
                    crate::service::inject_verify::VerifyOpts {
                        require_prompt_first: false,
                        verify_echo: enter,
                        verify_submit: enter,
                        ..Default::default()
                    },
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            },
        )
    }

    /// Write raw bytes (no bracket paste) — for keystrokes the TUI must interpret (vim j/k, arrows).
    pub fn write_raw(&self, pane_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .write(&pty_id, data)
            .map_err(|e| e.to_string())
    }

    /// Inject ordered raw key sequences off-thread so the state reactor never
    /// blocks on the pty lock. PTY-only — chatBackend panes have no keys.
    pub fn inject_raw_keys_async(&self, pane_id: &str, writes: Vec<Vec<u8>>) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            for bytes in writes {
                let _ = pty.lock().write(&pty_id, &bytes);
            }
        });
        Ok(())
    }

    /// PTY work runs off-thread so CodeTerm-originated senders return immediately.
    pub fn send_text_detached(&self, pane_id: &str, text: &str, enter: bool) {
        let svc = self.clone();
        let pane = pane_id.to_string();
        let text = text.to_string();
        std::thread::spawn(move || {
            if let Err(error) = svc.send_text(&pane, &text, enter) {
                log::warn!("[send-async] delivery to pane {pane} failed: {error}");
            }
        });
    }

    /// Feed payloads to the typed-launch input parser so the CLI/API path triggers
    /// the same `agent_type` lock as a direct Tauri write. Skips locked panes.
    pub(crate) fn feed_input_parser(&self, pty_id: &str, text: &str, enter: bool) {
        let (pane_id, locked) = {
            let m = self.mux.read();
            let pid = m.pane_id_for_pty(pty_id);
            let locked = pid
                .as_ref()
                .and_then(|id| m.get_pane(id))
                .map(|p| p.agent_type_locked)
                .unwrap_or(false);
            (pid, locked)
        };
        let Some(pane_id) = pane_id else { return };
        if locked {
            self.input_parser.clear(&pane_id);
            return;
        }
        let mut payload = text.as_bytes().to_vec();
        if enter {
            payload.push(b'\r');
        }
        if let Some(provider_id) = self.input_parser.process(&pane_id, &payload) {
            log::info!(
                "[input-parser] detected '{}' send in pane {} — locking agent_type",
                provider_id,
                pane_id,
            );
            self.set_and_lock_pane_agent_type(&pane_id, provider_id);
            self.emit_state_changed();
        }
    }
}

pub(crate) fn provider_or_generic_tui(provider_ready: Option<bool>, generic_ready: bool) -> bool {
    provider_ready.unwrap_or(generic_ready)
}

fn provider_frame_ready(
    tui_ready: bool,
    prompt_ready: bool,
    require_composer: bool,
    composer_ready: bool,
) -> bool {
    (tui_ready && (!require_composer || composer_ready)) || (!require_composer && prompt_ready)
}

#[cfg(test)]
mod planner_delivery_tests {
    use super::{provider_frame_ready, provider_or_generic_tui, submit_authenticated_pty_input};
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    fn pty_planner_input_spy_never_degrades_to_shell_write() {
        let (mut mux, pane_id) = MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.backing = crate::multiplexer::types::PaneBacking::Pty;
        pane.pty_id = Some("pty-planner".into());
        pane.chat_view = true;
        pane.agent_type = Some("codex".into());
        pane.agent_session_id = Some("session".into());
        pane.session_binding_nonce = Some("nonce".into());
        pane.reported_session_binding_nonce = Some("nonce".into());
        pane.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);

        let payload = "- audit `model`\n- <nonexistent>\n$env:X='y'; $(false)";
        let mut agent_input_spy = Vec::new();
        submit_authenticated_pty_input(pane, "session", "nonce", payload, |pty, provider, text| {
            agent_input_spy.push((pty.to_string(), provider.to_string(), text.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            agent_input_spy,
            [("pty-planner".into(), "codex".into(), payload.into())]
        );

        for shell in ["powershell", "bash", "zsh"] {
            pane.agent_type = None;
            pane.agent_session_id = None;
            pane.reported_session_binding_nonce = None;
            pane.agent_state = None;
            pane.boot_stage = Some(codeterm_types::pty::BootStage::ShellReady);
            let mut shell_write_spy = Vec::new();
            let result =
                submit_authenticated_pty_input(pane, "session", "nonce", payload, |_, _, text| {
                    shell_write_spy.push((shell, text.to_string()));
                    Ok(())
                });
            assert!(result.is_err());
            assert!(shell_write_spy.is_empty(), "{shell} received planner bytes");
        }
    }

    #[test]
    fn provider_readiness_overrides_generic_tui_glyphs() {
        assert!(!provider_or_generic_tui(Some(false), true));
        assert!(provider_or_generic_tui(Some(true), false));
        assert!(provider_or_generic_tui(None, true));
    }

    #[test]
    fn provider_prompt_proves_launch_without_claiming_composer_readiness() {
        assert!(provider_frame_ready(false, true, false, false));
        assert!(!provider_frame_ready(false, true, true, false));
        assert!(provider_frame_ready(true, false, true, true));
    }
}
