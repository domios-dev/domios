//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.
use super::ServiceContext;
use serde::Deserialize;
use std::collections::HashMap;

const SHELL_BACKEND_COLS: u16 = 120;
const SEND_AGENT_BACKEND_COLS: u16 = 240;

/// Build the byte sequence written to a freshly-attached pty to launch a
/// shell-level command (agent spawn, mem worker spawn, anything that goes
/// through bash/cmd to start a process). On Unix we write the launch_cmd
/// to a temp script and exec via `bash <path>` to bypass bracketed-paste
/// markers entirely.
/// macOS system bash 3.2 has no bracketed-paste readline binding, so the
/// markers leak as literal `00~` / `01~` text into the shell command and
/// break spawn (mem worker spawns silently broken on Intel Mac mini hosts
/// whose login shell is still /bin/bash). The script-file path is shell-
/// agnostic — markers stay only in the interactive Claude TUI input path
/// where they are still needed.
/// Script self-cleans via `trap 'rm -f $0' EXIT` so /tmp doesn't accumulate.
/// Errors propagate — no fallback to known-broken bracketed paste.
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

/// Windows pty + cmd.exe handle bracketed-paste markers correctly and there
/// is no /bin/bash to script-exec with, so we keep the original behavior.
///
/// A sacrificial leading space absorbs PSReadLine's first-byte race: the very
/// first content byte after the `ESC[200~` opener is dropped when the paste
/// arrives before the bracketed-paste handler is fully armed (the
/// `emove-Item` / `nset CLAUDECODE` launch glitch). PowerShell ignores leading
/// whitespace, so the space is harmless whether or not it gets eaten.
#[cfg(windows)]
pub(crate) fn build_spawn_dispatch(cmd: &str) -> std::io::Result<Vec<u8>> {
    Ok(format!("\x1b[200~ {}\x1b[201~\r", cmd).into_bytes())
}

/// Cap a provider's submit-verify timeout. Interactive sends keep verification
/// (codex/opencode need the escalating-Enter retry to actually submit) but must
/// not inherit the provider's full 15s timeout, which compounds across retries
/// into a minute-long stall on a busy TUI.
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
    /// Send text to PTY via bracket paste mode (ESC[200~ ... ESC[201~).
    /// Standard terminal paste protocol — all TUI apps handle it correctly.
    /// Prevents special chars from being interpreted as commands.
    /// PTY init takes 1-15s after agent_spawn returns (shell spawn fast,
    /// agent spawn waits for orchestrator's background task to spawn +
    /// link the PTY). resolve_pty_id is wrapped in a 15s retry so scripts
    /// can chain `spawn → send` without hitting "no PTY".
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            let staged = self.run_outbound_stages(pane_id, text.to_string());
            return self.chatbackend_send(&plugin_id, &session_id, &staged);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;

        self.send_text_to_pty_smart(&pty_id, text, enter)
    }

    /// Verified injection for agent panes, naive paste for shells. A
    /// freshly-booted TUI can swallow the naive path's single fire-and-forget
    /// `\r`, leaving the message typed-but-never-submitted (the chat composer
    /// "no reply" bug); the verified path re-sends Enter until the TUI input
    /// drains. Blocking — call from sync context or spawn_blocking.
    pub fn send_text_to_pty_smart(
        &self,
        pty_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        let (pane_id, agent_type) = {
            let mux = self.mux.write();
            match mux.pane_id_for_pty(pty_id) {
                Some(pid) => {
                    let at = mux.get_pane(&pid).and_then(|p| p.agent_type.clone());
                    (Some(pid), at)
                }
                None => (None, None),
            }
        };
        // A booting TUI silently swallows bracket-paste — wait it out, else the
        // chat composer's message vanishes with no error (web "no reply" bug).
        let was_booting = pane_id
            .as_ref()
            .map(|pid| {
                self.mux
                    .write()
                    .get_pane(pid)
                    .map(|p| p.agent_booting)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if let Some(ref pid) = pane_id {
            self.await_agent_boot_blocking(pid, std::time::Duration::from_secs(90))?;
        }
        // ONLY for a just-booted agent: the boot flag can clear before the TUI
        // finishes rendering its input (no-task spawns clear it right after the
        // launch command), so wait for the provider's TUI fingerprint. An
        // already-running provider is ready — skip this so a plain send isn't
        // delayed by a fingerprint that doesn't happen to match the idle screen.
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
            // Cap the verify-submit timeout for interactive sends. We DO need
            // verification — codex/opencode swallow a single `\r` while still
            // ingesting the bracket paste, so without the drain check + escalating
            // Enter the message sits typed-but-unsent. But the provider's own
            // timeout (codex 15s) × retries stalled every send for a minute, so
            // cap it short: the drain normally confirms in <1s; the cap only
            // bounds the busy-TUI worst case.
            let strategy = cap_submit_timeout(
                self.agent_registry
                    .with_provider(agent_id, |p| p.verify_submit_strategy())
                    .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None),
                std::time::Duration::from_secs(3),
            );
            let opts = crate::service::inject_verify::VerifyOpts {
                // Don't gate on poll_shell_prompt: a live TUI's last screen line
                // is its footer (claude's "⏵⏵ bypass permissions…"), not the ❯
                // input cursor, so the prompt poll would time out on a perfectly
                // ready agent. The boot-wait above handles the not-yet-ready race.
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

    /// Poll the pty's screen until the provider's TUI fingerprint shows (or the
    /// universal agent-TUI glyph check), bounded by `timeout`. Best-effort: a
    /// timeout just proceeds — the verified injection still runs.
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

    /// `send_text` that first waits out an in-flight agent boot. A booting
    /// TUI silently swallows bracket-paste, so input sent in that window
    /// vanished without an error — the chat composer's "message lost" bug.
    pub async fn send_text_when_ready(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            let staged = self.run_outbound_stages(pane_id, text.to_string());
            return self.chatbackend_send(&plugin_id, &session_id, &staged);
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            let booting = self
                .mux
                .write()
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

    fn await_agent_boot_blocking(
        &self,
        pane_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let booting = self
                .mux
                .write()
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

    fn pane_env(&self, pane_id: &str) -> HashMap<String, String> {
        let mut env_vars = HashMap::new();
        env_vars.insert("CODETERM_PANE_ID".into(), pane_id.to_string());
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(pane_id),
        );
        env_vars.insert(
            "CODETERM_SOCKET_PATH".into(),
            crate::socket::server::socket_display_path(),
        );
        let api_cfg = crate::config::load_config().api;
        let host = if api_cfg.host == "0.0.0.0" {
            "127.0.0.1".to_string()
        } else {
            api_cfg.host.clone()
        };
        env_vars.insert(
            "CODETERM_API_URL".into(),
            format!("http://{}:{}", host, api_cfg.port),
        );
        env_vars.insert("CODETERM_API_PORT".into(), api_cfg.port.to_string());
        if let Some(ref tok) = api_cfg.token {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }

        let mux = self.mux.write();
        if let Some(pane) = mux.get_pane(pane_id) {
            if let Some(ref role) = pane.role {
                env_vars.insert("CODETERM_ROLE".into(), format!("{:?}", role));
            }
            if let Some(ref parent) = pane.parent_pane_id {
                env_vars.insert("CODETERM_PARENT_PANE_ID".into(), parent.clone());
            }
            if let Some(ref worker_id) = pane.worker_id {
                env_vars.insert("CODETERM_WORKER_ID".into(), worker_id.clone());
            }
        }
        env_vars
    }

    fn start_backend_reader_for(&self, pty_id: String) {
        let svc_reader = std::sync::Arc::new(self.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_id,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => {
                        log::warn!(
                            "[ensure-pty] backend reader failed for {}: {}",
                            &pty_id[..pty_id.len().min(8)],
                            e
                        );
                    }
                }
            });
        });
    }

    fn restored_tab_context(&self, pane_id: &str) -> Option<(String, String)> {
        let mux = self.mux.write();
        for ws in &mux.snapshot().workspaces {
            for tab in &ws.tabs {
                if tab.pane_id == pane_id {
                    return Some((ws.id.clone(), tab.id.clone()));
                }
            }
        }
        None
    }

    pub(crate) fn ensure_pane_pty_for_send(&self, pane_id: &str) -> Result<(), String> {
        // [G] A chatBackend pane must NEVER spawn a shell PTY (B1.1#4). Its
        // session is opened by the spawn path; on send there is nothing to
        // ensure. Fixing this one function neutralizes all 9 callers.
        if self.pane_chat_backing(pane_id).is_some() {
            return Ok(());
        }
        let resolved = self
            .mux
            .write()
            .resolve_pane_id(pane_id)
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        if self.resolve_pty_id(&resolved).is_ok() {
            return Ok(());
        }

        let (cwd, protected, restored, agent_type, session_id, auto_approve) = {
            let mux = self.mux.write();
            let pane = mux
                .get_pane(&resolved)
                .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
            (
                pane.cwd.clone(),
                pane.protected,
                pane.restored_from_snapshot,
                pane.agent_type.clone(),
                pane.agent_session_id.clone(),
                pane.auto_approve,
            )
        };
        if protected {
            return Err(format!(
                "Pane {} is protected and its backend PTY is not live yet",
                pane_id
            ));
        }

        let env_vars = self.pane_env(&resolved);
        let cols = if agent_type.is_some() {
            SEND_AGENT_BACKEND_COLS
        } else {
            SHELL_BACKEND_COLS
        };
        let t0 = std::time::Instant::now();
        let built = crate::pty::manager::build_session(None, cwd.clone(), cols, 40, env_vars)?;
        let (pty_id, _) = self.pty.lock().register_session(None, built);
        log::info!(
            "[ensure-pty] pane {} activated backend pty {} (build={}ms)",
            resolved,
            &pty_id[..pty_id.len().min(8)],
            t0.elapsed().as_millis()
        );
        self.link_pty(&resolved, pty_id.clone());
        self.start_backend_reader_for(pty_id);

        if restored {
            if let (Some(provider_id), Some(session_id), Some((ws_id, tab_id))) =
                (agent_type, session_id, self.restored_tab_context(&resolved))
            {
                let session_valid = cwd
                    .as_deref()
                    .map(|dir| {
                        self.agent_registry
                            .with_provider(&provider_id, |p| p.session_exists(dir, &session_id))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if session_valid {
                    let params = crate::service::SpawnAgentParams {
                        agent: Some(provider_id.clone()),
                        cwd,
                        skip_permissions: Some(auto_approve),
                        resume_session_id: Some(session_id),
                        ..Default::default()
                    };
                    let launch_cmd = crate::service::orchestrator::build_agent_command(
                        &provider_id,
                        &params,
                        None,
                        None,
                        &self.agent_registry,
                    );
                    crate::service::orchestrator::launch_agent_in_background(
                        self.clone(),
                        resolved.clone(),
                        tab_id,
                        ws_id,
                        launch_cmd,
                        provider_id,
                        None,
                        None,
                    );
                    self.clear_pane_restored_flag(&resolved);
                }
            }
        }
        Ok(())
    }

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

    /// Windows ConPTY: writing a launch command before the shell prompt is
    /// rendered races the first byte — it can be dropped or duplicated, yielding
    /// a mangled command line (e.g. `c codex`, which PowerShell rejects with
    /// CommandNotFoundException). Block (bounded) until the prompt appears so
    /// PSReadLine's bracketed-paste handler is armed before the write. Native
    /// PTYs on macOS/Linux don't exhibit this race, so this is a no-op there.
    fn await_shell_ready(&self, pty_id: &str) {
        #[cfg(windows)]
        poll_shell_prompt(
            &self.screen_state,
            pty_id,
            std::time::Duration::from_secs(10),
        );
        #[cfg(not(windows))]
        let _ = pty_id;
    }

    fn await_pty_link_blocking(
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

    /// Same as [`send_text`] but takes a PTY id directly, skipping pane→pty
    /// resolution. Use this when the caller already holds a verified pty_id
    /// and must not race against `link_pty` overwrites swapping `pane.pty_id`
    /// out from under the send.
    pub fn send_text_to_pty(&self, pty_id: &str, text: &str, enter: bool) -> Result<(), String> {
        // Feed parsed text before bracket-paste ESCs so parser state stays stable.
        // This keeps API/CLI launched commands reliable when a launch is typed manually.
        self.feed_input_parser(pty_id, text, enter);
        let paste = if cfg!(windows) {
            format!("\x1b[200~ {}\x1b[201~", text)
        } else {
            format!("\x1b[200~{}\x1b[201~", text)
        };
        self.pty.lock().write(pty_id, paste.as_bytes())?;

        if enter {
            // Send \r as a separate raw write after a short delay. TUI apps (Claude Code/ink) process bracket paste asynchronously — \r concatenated with
            // paste end or sent immediately is swallowed.
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
        super::inject_safety::notify_if_unsafe_prompt(&self.screen_state, pty_id);

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

    /// Write raw bytes to PTY without bracket paste wrapping.
    /// Use for keystrokes that need TUI interpretation (vim j/k, arrow keys, etc).
    pub fn write_raw(&self, pane_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .write(&pty_id, data)
            .map_err(|e| e.to_string())
    }

    /// Feed `send_text`-style payloads to the typed-launch input parser so the
    /// CLI/API path triggers the same `agent_type` lock as a direct Tauri write.
    /// Skips locked panes; on match, sets and locks the detected provider.
    fn feed_input_parser(&self, pty_id: &str, text: &str, enter: bool) {
        let (pane_id, locked) = {
            let m = self.mux.write();
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

    pub fn send_text_batch(&self, pane_id: &str, batch: Vec<SendBatchItem>) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            for item in batch {
                let staged = self.run_outbound_stages(pane_id, item.text);
                self.chatbackend_send(&plugin_id, &session_id, &staged)?;
            }
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(pane_id)?;
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

    pub fn resize_pane(&self, pane_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        // A chatBackend pane has no terminal to resize — graceful no-op.
        if self.pane_chat_backing(pane_id).is_some() {
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .resize(&pty_id, cols, rows)
            .map_err(|e| e.to_string())
    }

    /// Kill the PTY process for a pane without removing the pane from the mux.
    /// Different from `close_pane`, which both kills the PTY and removes the pane.
    pub fn kill_pane_pty(&self, pane_id: &str) -> Result<(), String> {
        // A chatBackend pane has no PTY process — close the plugin session.
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            return self.chatbackend_close(&plugin_id, &session_id);
        }
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty.lock().remove(&pty_id);
        Ok(())
    }

    /// Resolve pane_id (or alias) → pty_id.
    pub fn resolve_pty_id(&self, pane_id: &str) -> Result<String, String> {
        let mux = self.mux.write();
        let resolved = mux
            .resolve_pane_id(pane_id)
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        let pty_id = mux
            .get_pane(&resolved)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        drop(mux);
        if self.pty.lock().get_session(&pty_id).is_none() {
            return Err(format!("Pane {} PTY is not live: {}", pane_id, pty_id));
        }
        Ok(pty_id)
    }

    /// Wait for a pane to get a PTY assigned by the frontend. Polls with timeout.
    pub async fn wait_for_pty(&self, pane_id: &str, timeout_secs: u64) -> Result<String, String> {
        let mux = self.mux.clone();
        let pid = pane_id.to_string();
        tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            loop {
                {
                    let m = mux.write();
                    if let Some(pane) = m.get_pane(&pid) {
                        if let Some(ref pty_id) = pane.pty_id {
                            return pty_id.clone();
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| "Timeout waiting for PTY".to_string())
    }

    /// Wait for output buffer to grow by at least `min_bytes`.
    pub async fn wait_for_output_growth(&self, pty_id: &str, min_bytes: usize, timeout_secs: u64) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async move {
            let start_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
            loop {
                let cur_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
                if cur_len > start_len + min_bytes {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        })
        .await;
    }
}

/// Wait (event-driven) for a shell prompt to appear on a PTY.
/// Arms the prompt signal once under the `screen_state` lock and then blocks on a
/// condvar — no busy-poll. The `ScreenState::process` path fires the signal as soon
/// as a chunk produces a prompt-looking last line, so this returns immediately.
/// Returns `true` if ready, `false` on timeout.
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

#[derive(Debug, Clone, Deserialize)]
pub struct SendBatchItem {
    pub text: String,
    #[serde(default)]
    pub enter: bool,
    pub delay_ms: Option<u64>,
}

#[cfg(test)]
mod tests;
