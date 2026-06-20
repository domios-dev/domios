//! Interactive send path: build_spawn_dispatch, poll_shell_prompt, and all
//! send/write/batch ServiceContext methods.
use crate::service::ServiceContext;

/// Bytes written to a fresh pty to launch a shell-level command. On Unix, exec a
/// temp script via `bash <path>` to bypass bracketed-paste markers — macOS system
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
    Ok(format!("bash {}\r", script_path).into_bytes())
}

/// Windows keeps bracketed paste (no /bin/bash to script-exec). The leading space
/// is sacrificial: it absorbs PSReadLine's dropped first byte before the paste handler arms.
#[cfg(windows)]
pub(crate) fn build_spawn_dispatch(cmd: &str) -> std::io::Result<Vec<u8>> {
    Ok(format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes())
}

/// Wait (event-driven, via a condvar armed under `screen_state`) for a shell
/// prompt to appear on a PTY. Returns `true` if ready, `false` on timeout.
pub fn poll_shell_prompt(
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let signal = screen_state.lock().arm_prompt_signal(pty_id);
    let short_id = &pty_id[..pty_id.len().min(8)];
    let t0 = std::time::Instant::now();
    let ready = signal.wait(timeout);
    if ready {
        log::info!(
            "[poll_shell_prompt] shell ready for pty {} after {}ms",
            short_id,
            t0.elapsed().as_millis()
        );
    } else {
        log::warn!(
            "[poll_shell_prompt] timeout after {}ms for pty {}",
            timeout.as_millis(),
            short_id
        );
    }
    ready
}

/// Cap a provider's submit-verify timeout: keep verification but don't let the
/// full provider timeout compound across retries into a minute-long stall.
fn cap_submit_timeout(
    strategy: crate::agent::provider::VerifySubmitStrategy,
    max: std::time::Duration,
) -> crate::agent::provider::VerifySubmitStrategy {
    use crate::agent::provider::VerifySubmitStrategy as S;
    match strategy {
        S::ShellPrompt { timeout } => S::ShellPrompt {
            timeout: timeout.min(max),
        },
        S::TuiInputDrained {
            timeout,
            idle_after_drain,
        } => S::TuiInputDrained {
            timeout: timeout.min(max),
            idle_after_drain,
        },
        S::None => S::None,
    }
}

impl ServiceContext {
    /// Send text to a pane (chatBackend or PTY). PTY link is awaited up to 15s so
    /// scripts can chain `spawn → send` without hitting "no PTY".
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        if self.pane_chat_backing(pane_id).is_some() {
            return self.chatbackend_pane_send(pane_id, text);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;

        self.send_text_to_pty_smart(&pty_id, text, enter)
    }

    /// Verified injection for agent panes, naive paste for shells: a booting TUI
    /// can swallow the naive `\r`, so the verified path re-sends Enter until the
    /// input drains (the "no reply" bug). Blocking — call from sync/spawn_blocking.
    pub fn send_text_to_pty_smart(
        &self,
        pty_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        let (pane_id, agent_type) = {
            let mux = self.mux.read();
            match mux.pane_id_for_pty(pty_id) {
                Some(pid) => {
                    let at = mux.get_pane(&pid).and_then(|p| p.agent_type.clone());
                    (Some(pid), at)
                }
                None => (None, None),
            }
        };
        // A booting TUI silently swallows bracket-paste — wait it out, else the message vanishes silently.
        let was_booting = pane_id
            .as_ref()
            .map(|pid| {
                self.mux
                    .read()
                    .get_pane(pid)
                    .map(|p| p.agent_booting)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if let Some(ref pid) = pane_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))?;
        }
        // Only for a just-booted agent: the boot flag can clear before the TUI renders its input, so wait for its fingerprint.
        if was_booting {
            if let Some(ref agent_id) = agent_type {
                self.await_agent_tui_on_screen(
                    pty_id,
                    agent_id,
                    std::time::Duration::from_secs(10),
                );
            }
        }
        if let Some(ref agent_id) = agent_type {
            // codex/opencode swallow a `\r` while ingesting the paste, so verification stays — but capped so retries don't stall a minute.
            let strategy = cap_submit_timeout(
                self.agent_registry
                    .with_provider(agent_id, |p| p.verify_submit_strategy())
                    .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None),
                std::time::Duration::from_secs(3),
            );
            let opts = crate::service::inject_verify::VerifyOpts {
                // Don't gate on poll_shell_prompt: a live TUI's last line is its footer, not a prompt, so the poll would time out.
                require_prompt_first: false,
                verify_echo: false,
                verify_submit: true,
                ..Default::default()
            };
            return self
                .send_text_verified(pty_id, text, enter, strategy, opts)
                .map(|_| ())
                .map_err(|e| e.to_string());
        }
        self.send_text_to_pty(pty_id, text, enter)
    }

    /// Poll the screen until the provider's TUI fingerprint shows, bounded by `timeout`. Best-effort.
    fn await_agent_tui_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let screen = self.screen_state.lock().get_screen_text(pty_id);
            if let Some(text) = screen {
                let ready = crate::pty::screen_state::screen_has_agent_tui(&text)
                    || self
                        .agent_registry
                        .with_provider(agent_id, |p| p.screen_has_tui(&text))
                        .unwrap_or(false);
                if ready {
                    return;
                }
            }
            if std::time::Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    /// `send_text` that first waits out an in-flight agent boot — a booting TUI swallows input silently.
    pub async fn send_text_when_ready(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        if self.pane_chat_backing(pane_id).is_some() {
            return self.chatbackend_pane_send(pane_id, text);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            let booting = self
                .mux
                .read()
                .get_pane(pane_id)
                .map(|p| p.agent_booting)
                .unwrap_or(false);
            if !booting {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "Agent in pane {} is still booting — input would be lost",
                    pane_id
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        self.send_text(pane_id, text, enter)
    }

    pub(crate) fn await_agent_boot_blocking(
        &self,
        pane_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let booting = self
                .mux
                .read()
                .get_pane(pane_id)
                .map(|p| p.agent_booting)
                .unwrap_or(false);
            if !booting {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "Agent in pane {} is still booting — input would be lost",
                    pane_id
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    /// Windows ConPTY: a write before the prompt renders races the first byte and
    /// mangles the command, so block until the prompt arms PSReadLine. No-op on Unix.
    pub(crate) fn await_shell_ready(&self, pty_id: &str) {
        #[cfg(windows)]
        poll_shell_prompt(
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
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Ok(pty_id) = self.resolve_pty_id(pane_id) {
                return Ok(pty_id);
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "Pane {} not found or PTY did not start within {:?}",
                    pane_id, timeout
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    /// `send_text` by PTY id, skipping pane→pty resolution — use when the caller
    /// holds a verified pty_id and must not race a `link_pty` swap of `pane.pty_id`.
    pub fn send_text_to_pty(&self, pty_id: &str, text: &str, enter: bool) -> Result<(), String> {
        // Feed parsed text before the bracket-paste ESCs so parser state stays stable.
        self.feed_input_parser(pty_id, text, enter);
        let paste = if cfg!(windows) {
            format!("\x1b[200~ {}\x1b[201~", text)
        } else {
            format!("\x1b[200~{}\x1b[201~", text)
        };
        self.pty.lock().write(pty_id, paste.as_bytes())?;

        if enter {
            // \r as a separate delayed write: ink-based TUIs process bracket paste async and swallow a \r sent with it.
            let pty = self.pty.clone();
            let pty_id = pty_id.to_string();
            if tokio::runtime::Handle::try_current().is_ok() {
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    let _ = pty.lock().write(&pty_id, b"\r");
                });
            } else {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = pty.lock().write(&pty_id, b"\r");
                });
            }
        }
        Ok(())
    }

    /// Write raw bytes (no bracket paste) — for keystrokes the TUI must interpret (vim j/k, arrows).
    pub fn write_raw(&self, pane_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .write(&pty_id, data)
            .map_err(|e| e.to_string())
    }

    /// Inject ordered raw key sequences off-thread so the caller (state reactor's
    /// poll loop) never blocks on the pty lock. PTY-only — chatBackend panes have no keys.
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

    pub fn send_text_batch(
        &self,
        pane_id: &str,
        batch: Vec<super::SendBatchItem>,
    ) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            for item in batch {
                let staged = self.run_outbound_stages(pane_id, item.text);
                self.chatbackend_send(&plugin_id, &session_id, &staged)?;
            }
            return Ok(());
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;
        let pty = self.pty.clone();
        tokio::spawn(async move {
            for item in batch {
                if let Some(delay) = item.delay_ms {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                let text = if item.enter && !item.text.ends_with('\r') {
                    format!("{}\r", item.text)
                } else {
                    item.text
                };
                let _ = pty.lock().write(&pty_id, text.as_bytes());
            }
        });
        Ok(())
    }
}
