//! PTY I/O helpers: send text, batch send, resize, wait-for-pty.

use serde::Deserialize;
use super::ServiceContext;

impl ServiceContext {
    /// Send text to PTY via bracket paste mode (ESC[200~ ... ESC[201~).
    /// Standard terminal paste protocol — all TUI apps handle it correctly.
    /// Prevents special chars from being interpreted as commands.
    pub fn send_text(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(pane_id)?;
        let mut buf = String::new();
        buf.push_str("\x1b[200~");
        buf.push_str(text);
        buf.push_str("\x1b[201~");
        if enter {
            buf.push('\r');
        }
        self.pty
            .lock()
            .write(&pty_id, buf.as_bytes())
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

/// Poll output buffer for shell prompt markers. Returns true if found within timeout.
pub fn poll_shell_prompt(output_buf: &crate::OutputBufferState, pty_id: &str, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() > deadline { return false; }
        let ready = output_buf.try_lock()
            .and_then(|buf| buf.get_text(pty_id).map(|t| {
                t.contains("$ ") || t.contains("% ") || t.contains("> ") || t.contains("PS ")
            }))
            .unwrap_or(false);
        if ready { return true; }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
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
}
