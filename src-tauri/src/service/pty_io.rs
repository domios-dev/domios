//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.

use serde::Deserialize;
use super::ServiceContext;

/// Build the byte sequence written to a freshly-attached pty to launch a
/// shell-level command (agent spawn, mem worker spawn, anything that goes
/// through bash/cmd to start a process). On Unix we write the launch_cmd
/// to a temp script and exec via `bash <path>` to bypass bracketed-paste
/// markers entirely.
///
/// macOS system bash 3.2 has no bracketed-paste readline binding, so the
/// markers leak as literal `00~` / `01~` text into the shell command and
/// break spawn (mem worker spawns silently broken on Intel Mac mini hosts
/// whose login shell is still /bin/bash). The script-file path is shell-
/// agnostic — markers stay only in the interactive Claude TUI input path
/// where they are still needed.
///
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
#[cfg(windows)]
pub(crate) fn build_spawn_dispatch(cmd: &str) -> std::io::Result<Vec<u8>> {
    Ok(format!("\x1b[200~{}\x1b[201~\r", cmd).into_bytes())
}

impl ServiceContext {
    /// Send text to PTY via bracket paste mode (ESC[200~ ... ESC[201~).
    /// Standard terminal paste protocol — all TUI apps handle it correctly.
    /// Prevents special chars from being interpreted as commands.
    /// PTY init takes 1-15s after agent_spawn returns (shell spawn fast,
    /// agent spawn waits for orchestrator's background task to spawn +
    /// link the PTY). resolve_pty_id is wrapped in a 15s retry so scripts
    /// can chain `spawn → send-text` without hitting "no PTY".
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;
        self.send_text_to_pty(&pty_id, text, enter)
    }

    /// Launch a shell-level command in an attached pty (agent spawn, mem worker
    /// spawn, etc). Bypasses bracketed-paste — see `build_spawn_dispatch` for
    /// the rationale (bash 3.2 cannot interpret paste markers, the launch
    /// pipeline must arrive as a normal shell command line). Distinct from the
    /// interactive TUI input paths (`send_text_*`), which still use bracketed
    /// paste because Claude / Codex TUIs require it.
    pub fn send_shell_spawn(&self, pty_id: &str, cmd: &str) -> Result<(), String> {
        let bytes = build_spawn_dispatch(cmd).map_err(|e| e.to_string())?;
        self.pty.lock().write(pty_id, &bytes).map_err(|e| e.to_string())?;
        Ok(())
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
                return Err(format!("Pane {} not found or PTY did not start within {:?}", pane_id, timeout));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    /// Same as [`send_text`] but takes a PTY id directly, skipping pane→pty
    /// resolution. Use this when the caller already holds a verified pty_id
    /// and must not race against `link_pty` overwrites swapping `pane.pty_id`
    /// out from under the send.
    pub fn send_text_to_pty(&self, pty_id: &str, text: &str, enter: bool) -> Result<(), String> {
        let paste = format!("\x1b[200~{}\x1b[201~", text);
        self.pty.lock().write(pty_id, paste.as_bytes())?;

        if enter {
            // Send \r as a separate raw write after a short delay.
            // TUI apps (Claude Code/ink) process bracket paste asynchronously —
            // \r concatenated with paste end or sent immediately is swallowed.
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
    ) -> Result<crate::service::inject_verify::InjectionResult, crate::service::inject_verify::InjectionError> {
        use crate::agent::provider::VerifySubmitStrategy;
        use crate::service::inject_verify::{
            HelperErr, InjectionError, InjectionResult, snapshot_screen_hash, wait_for_echo,
            wait_for_submit_shell, wait_for_submit_tui_drained,
        };

        let started = std::time::Instant::now();
        let tail_rows: usize = 8;

        if opts.require_prompt_first {
            let ready = poll_shell_prompt(&self.screen_state, pty_id, opts.prompt_timeout);
            if !ready {
                return Err(InjectionError::PromptNeverReady { waited: started.elapsed() });
            }
        }

        let hash_before = snapshot_screen_hash(&self.screen_state, pty_id).unwrap_or_default();

        let paste = format!("\x1b[200~{}\x1b[201~", text);
        self.pty
            .lock()
            .write(pty_id, paste.as_bytes())
            .map_err(|_| InjectionError::PtyClosed { pty_id: pty_id.to_string() })?;
        let bytes_written = paste.len();

        // Echo verify: informational only. Timeout means we couldn't confirm,
        // not that the paste failed. PtyUnknown is the only real failure here
        // and surfaces as PtyClosed.
        let echo_observed = if opts.verify_echo {
            match wait_for_echo(&self.screen_state, pty_id, text, opts.echo_timeout, tail_rows) {
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

        // Enter ALWAYS fires when the caller asked for it. This is the
        // load-bearing line — no early-return above this point can skip it
        // unless the PTY is already gone.
        let submit_observed = if enter {
            // Brief settle: TUIs (claude/codex/opencode) buffer bracket paste async;
            // a same-instant \r can be eaten.
            std::thread::sleep(std::time::Duration::from_millis(120));
            self.pty
                .lock()
                .write(pty_id, b"\r")
                .map_err(|_| InjectionError::PtyClosed { pty_id: pty_id.to_string() })?;

            // Submit verify: also informational. Timeout no longer aborts.
            if opts.verify_submit {
                let res = match submit_strategy {
                    VerifySubmitStrategy::ShellPrompt { timeout } => wait_for_submit_shell(
                        &self.screen_state,
                        pty_id,
                        hash_before,
                        timeout,
                        tail_rows,
                    ),
                    VerifySubmitStrategy::TuiInputDrained { timeout, idle_after_drain } => {
                        wait_for_submit_tui_drained(
                            &self.screen_state,
                            pty_id,
                            text,
                            timeout,
                            idle_after_drain,
                            tail_rows,
                        )
                    }
                    VerifySubmitStrategy::None => Ok(String::new()),
                };
                match res {
                    Ok(_) => true,
                    Err(HelperErr::PtyUnknown(id)) => {
                        return Err(InjectionError::PtyClosed { pty_id: id });
                    }
                    Err(HelperErr::Timeout(_)) => {
                        log::debug!(
                            "submit verify timed out for pty {}; Enter was sent, recipient may still be processing",
                            pty_id
                        );
                        false
                    }
                }
            } else {
                false
            }
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

    /// Write raw bytes to PTY without bracket paste wrapping.
    /// Use for keystrokes that need TUI interpretation (vim j/k, arrow keys, etc).
    pub fn write_raw(&self, pane_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty.lock().write(&pty_id, data).map_err(|e| e.to_string())
    }

    pub fn send_text_batch(&self, pane_id: &str, batch: Vec<SendBatchItem>) -> Result<(), String> {
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
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty
            .lock()
            .resize(&pty_id, cols, rows)
            .map_err(|e| e.to_string())
    }

    /// Kill the PTY process for a pane without removing the pane from the mux.
    /// Different from `close_pane`, which both kills the PTY and removes the pane.
    pub fn kill_pane_pty(&self, pane_id: &str) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        self.pty.lock().remove(&pty_id);
        Ok(())
    }

    /// Resolve pane_id (or alias) → pty_id.
    pub fn resolve_pty_id(&self, pane_id: &str) -> Result<String, String> {
        let mux = self.mux.lock();
        let resolved = mux.resolve_pane_id(pane_id)
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))?;
        mux.get_pane(&resolved)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| format!("Pane {} not found or has no PTY", pane_id))
    }

    /// Wait for a pane to get a PTY assigned by the frontend. Polls with timeout.
    pub async fn wait_for_pty(&self, pane_id: &str, timeout_secs: u64) -> Result<String, String> {
        let mux = self.mux.clone();
        let pid = pane_id.to_string();
        tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            loop {
                {
                    let m = mux.lock();
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
    pub async fn wait_for_output_growth(
        &self,
        pty_id: &str,
        min_bytes: usize,
        timeout_secs: u64,
    ) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            async move {
                let start_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
                loop {
                    let cur_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
                    if cur_len > start_len + min_bytes {
                        return;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            },
        )
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
mod tests {
    use super::*;

    // ── Helper: captures the enter-append logic from send_text / send_text_batch ──
    // This mirrors the exact logic in ServiceContext::send_text
    fn apply_enter(text: &str, enter: bool) -> String {
        if enter && !text.ends_with('\r') && !text.ends_with('\n') {
            format!("{}\r", text)
        } else {
            text.to_string()
        }
    }

    // ── 1. send_text enter logic ──

    #[test]
    fn enter_true_appends_cr_when_missing() {
        assert_eq!(apply_enter("ls -la", true), "ls -la\r");
    }

    #[test]
    fn enter_true_no_double_cr() {
        // Text already ends with \r — should NOT append another
        assert_eq!(apply_enter("ls -la\r", true), "ls -la\r");
    }

    #[test]
    fn enter_false_passthrough() {
        assert_eq!(apply_enter("partial text", false), "partial text");
    }

    #[test]
    fn enter_false_preserves_trailing_cr() {
        assert_eq!(apply_enter("text\r", false), "text\r");
    }

    #[test]
    fn empty_text_enter_true_sends_just_cr() {
        assert_eq!(apply_enter("", true), "\r");
    }

    #[test]
    fn empty_text_enter_false_sends_nothing() {
        assert_eq!(apply_enter("", false), "");
    }

    #[test]
    fn enter_true_text_ending_newline() {
        // Text ending with \n already has a line terminator — no \r appended.
        assert_eq!(apply_enter("echo hello\n", true), "echo hello\n");
    }

    #[test]
    fn text_with_embedded_cr_but_not_trailing() {
        assert_eq!(apply_enter("line1\rline2", true), "line1\rline2\r");
    }

    #[test]
    fn multiline_text_enter_true() {
        assert_eq!(apply_enter("line1\nline2", true), "line1\nline2\r");
    }

    // ── SendBatchItem deserialization ──

    #[test]
    fn batch_item_enter_defaults_false() {
        // SendBatchItem uses #[serde(default)] which defaults bool to false
        let json = r#"{"text": "hello"}"#;
        let item: SendBatchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.text, "hello");
        assert_eq!(item.enter, false, "SendBatchItem.enter defaults to false (serde default)");
        assert!(item.delay_ms.is_none());
    }

    #[test]
    fn batch_item_enter_explicit_true() {
        let json = r#"{"text": "cmd", "enter": true, "delay_ms": 100}"#;
        let item: SendBatchItem = serde_json::from_str(json).unwrap();
        assert!(item.enter);
        assert_eq!(item.delay_ms, Some(100));
    }

    #[test]
    fn batch_item_enter_explicit_false() {
        let json = r#"{"text": "partial", "enter": false}"#;
        let item: SendBatchItem = serde_json::from_str(json).unwrap();
        assert!(!item.enter);
    }

    // ── screen_has_shell_prompt ──

    use crate::pty::screen_state::screen_has_shell_prompt;

    #[test]
    fn prompt_bash_dollar() {
        assert!(screen_has_shell_prompt("user@host:~$ "));
    }

    #[test]
    fn prompt_bash_dollar_no_trailing_space() {
        assert!(screen_has_shell_prompt("user@host:~$"));
    }

    #[test]
    fn prompt_zsh_percent() {
        assert!(screen_has_shell_prompt("user@host % "));
    }

    #[test]
    fn prompt_powershell() {
        assert!(screen_has_shell_prompt("PS C:\\Users\\User> "));
    }

    #[test]
    fn prompt_powershell_no_trailing_space() {
        assert!(screen_has_shell_prompt("PS C:\\Users\\User>"));
    }

    #[test]
    fn prompt_generic_angle() {
        assert!(screen_has_shell_prompt("prompt> "));
    }

    #[test]
    fn prompt_last_nonempty_line() {
        let screen = "some output\nmore output\nuser@host:~$ \n\n";
        assert!(screen_has_shell_prompt(screen));
    }

    #[test]
    fn no_prompt_in_file_content() {
        let screen = "Reading file.txt:\ncontains PS data and > markers\nstill loading...";
        assert!(!screen_has_shell_prompt(screen));
    }

    #[test]
    fn no_prompt_empty_screen() {
        assert!(!screen_has_shell_prompt(""));
        assert!(!screen_has_shell_prompt("\n\n\n"));
    }

    #[test]
    fn no_false_positive_mid_line_dollar() {
        let screen = "echo $HOME is /home/user\nprocessing...";
        assert!(!screen_has_shell_prompt(screen));
    }
}
