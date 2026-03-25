//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.

use serde::Deserialize;
use super::ServiceContext;

impl ServiceContext {
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        let data = if enter && !text.ends_with('\r') {
            format!("{}\r", text)
        } else {
            text.to_string()
        };
        self.pty
            .lock()
            .write(&pty_id, data.as_bytes())
            .map_err(|e| e.to_string())
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

    /// Resolve pane_id → pty_id.
    pub fn resolve_pty_id(&self, pane_id: &str) -> Result<String, String> {
        let mux = self.mux.lock();
        mux.get_pane(pane_id)
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

    /// Wait for shell prompt to appear in output buffer.
    ///
    /// Checks output buffer for common shell prompt markers. If timeout
    /// is reached without finding a prompt, proceeds anyway — the command
    /// will queue in the PTY input buffer until the shell is ready.
    pub async fn wait_for_shell_prompt(&self, pty_id: &str, timeout_secs: u64) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            async move {
                loop {
                    let ready = {
                        let buf = ob.lock();
                        buf.get_text(&pid)
                            .map(|t| {
                                t.contains("PS ") || t.contains("> ")
                                    || t.contains("$ ") || t.contains("% ")
                            })
                            .unwrap_or(false)
                    };
                    if ready {
                        return;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                }
            },
        )
        .await;
        if result.is_err() {
            log::warn!("wait_for_shell_prompt: timeout for PTY {}, proceeding anyway", pty_id);
        }
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

    /// Spawn a background thread that polls for Claude Code readiness, then sends task.
    ///
    /// Detects readiness by looking for Claude Code UI markers in the terminal
    /// output buffer tail. Handles auto-updates by continuing to poll until
    /// the interactive prompt appears.
    pub fn spawn_task_sender(&self, pty_id: String, task: String) {
        let ob = self.output_buf.clone();
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            // 3 minutes: accounts for auto-updates, slow networks, large project indexing
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
            let mut last_len = 0usize;

            loop {
                if std::time::Instant::now() > deadline {
                    log::warn!("spawn_task_sender: timeout waiting for agent readiness ({})", pty_id);
                    break;
                }
                // Use try_lock to avoid blocking other threads that need output_buf
                if let Some(buf) = ob.try_lock() {
                    if let Some(data) = buf.get_text(&pty_id) {
                        let cur_len = data.len();
                        // Only check when new output arrives
                        if cur_len > last_len {
                            last_len = cur_len;
                            let tail = if data.len() > 1000 {
                                &data[data.len() - 1000..]
                            } else {
                                &data
                            };
                            // Claude Code readiness markers (any of these means prompt is active)
                            if is_claude_ready(tail) {
                                drop(buf);
                                // Retry loop: send task, check if it landed, retry if not
                                for attempt in 0..3 {
                                    std::thread::sleep(std::time::Duration::from_secs(if attempt == 0 { 2 } else { 3 }));
                                    let _ = pty.lock().write(&pty_id, format!("{}\r", task).as_bytes());
                                    std::thread::sleep(std::time::Duration::from_secs(1));
                                    let _ = pty.lock().write(&pty_id, b"\r");

                                    // Verify task was accepted — check if output grew significantly
                                    std::thread::sleep(std::time::Duration::from_secs(3));
                                    let accepted = ob.try_lock()
                                        .and_then(|b| b.get_text(&pty_id).map(|t| {
                                            // If Claude started working, we'll see tool calls or thinking markers
                                            t.len() > last_len + 100
                                        }))
                                        .unwrap_or(false);

                                    if accepted {
                                        log::info!("spawn_task_sender: task accepted on attempt {} for {}", attempt + 1, pty_id);
                                        return;
                                    }
                                    log::warn!("spawn_task_sender: task may not have landed (attempt {}), retrying for {}", attempt + 1, pty_id);
                                }
                                log::info!("spawn_task_sender: task sent (3 attempts) to {}", pty_id);
                                return;
                            }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }
}

/// Check if Claude Code interactive prompt is ready for input.
fn is_claude_ready(output_tail: &str) -> bool {
    // These markers appear in the Claude Code TUI when the input prompt is active
    output_tail.contains("shift+tab")
        || output_tail.contains("hold Space")
        || output_tail.contains("esc to interrupt")
        || output_tail.contains("ctrl+g to edit")
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendBatchItem {
    pub text: String,
    #[serde(default)]
    pub enter: bool,
    pub delay_ms: Option<u64>,
}
