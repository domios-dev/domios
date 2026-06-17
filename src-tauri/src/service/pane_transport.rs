//! `PaneTransport` — the single pane-I/O seam (plan Appendix B1.1#2).
//!
//! The handful of *pane-keyed* entry operations (send / read transcript /
//! resize / kill) dispatch on the pane's [`PaneBacking`] instead of calling
//! `resolve_pty_id` directly. Two impls:
//!
//! * [`PtyTransport`] — today's PTY path, delegating to the existing
//!   `ServiceContext` methods verbatim (no behaviour change for PTY panes).
//! * [`ChatBackendTransport`] — routes to the plugin `chatBackend` registry
//!   (added in B1.3 #3).
//!
//! Entry points call `svc.pane_transport(pane_id)?.send(...)`, which collapses
//! the ~20 scattered backing branches into one dispatch + two impls. The CI
//! grep guard (B1.1#7) asserts no NEW `resolve_pty_id` call is added outside
//! this module.

use super::pty_io::SendBatchItem;
use super::ServiceContext;
use crate::multiplexer::types::PaneBacking;

/// Transport-agnostic pane I/O. One impl per [`PaneBacking`].
pub trait PaneTransport {
    /// Send user text to the pane (PTY paste, or plugin `sendMessage`).
    fn send(&self, text: &str, enter: bool) -> Result<(), String>;
    /// Send a delayed/scripted batch.
    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String>;
    /// Resize the underlying terminal. No-op for non-PTY transports.
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String>;
    /// Kill the backing process/session without removing the pane.
    fn kill(&self) -> Result<(), String>;
    /// `codeterm get-output` — buffered output text (PTY) or transcript
    /// (chatBackend). Returns `{ output, offset }`.
    fn read_output(
        &self,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String>;
    /// `codeterm get-text` — current screen text (PTY) or transcript
    /// (chatBackend). Returns `{ text, formatted }`.
    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String>;
}

/// Advance `idx` to the next UTF-8 char boundary in `s` (clamped to `s.len()`).
pub(crate) fn next_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// PTY-backed transport. Delegates to the existing `ServiceContext` PTY methods
/// so behaviour is byte-identical to the pre-seam path.
pub struct PtyTransport<'a> {
    pub(crate) svc: &'a ServiceContext,
    pub(crate) pane_id: String,
}

impl PaneTransport for PtyTransport<'_> {
    fn send(&self, text: &str, enter: bool) -> Result<(), String> {
        self.svc.send_text(&self.pane_id, text, enter)
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        self.svc.send_text_batch(&self.pane_id, batch)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.svc.resize_pane(&self.pane_id, cols, rows)
    }

    fn kill(&self) -> Result<(), String> {
        self.svc.kill_pane_pty(&self.pane_id)
    }

    fn read_output(
        &self,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String> {
        let pty_id = self.svc.resolve_pty_id(&self.pane_id)?;
        let buf = self.svc.output_buf.lock();
        let text = if full {
            buf.get_full_text(&pty_id).unwrap_or_default()
        } else {
            buf.get_text(&pty_id).unwrap_or_default()
        };
        let start = next_char_boundary(&text, offset.unwrap_or(0));
        let raw = &text[start..];
        let truncated = if let Some(max) = max_bytes {
            if raw.len() > max {
                let tail_start = next_char_boundary(raw, raw.len() - max);
                &raw[tail_start..]
            } else {
                raw
            }
        } else {
            raw
        };
        let output = if strip_ansi {
            crate::pty::ansi::clean_terminal_output(truncated)
        } else {
            truncated.to_string()
        };
        Ok(serde_json::json!({ "output": output, "offset": text.len() }))
    }

    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String> {
        let pty_id = self.svc.resolve_pty_id(&self.pane_id)?;
        let ss = self.svc.screen_state.lock();
        let text = if formatted {
            let bytes = ss
                .get_screen_formatted(&pty_id)
                .ok_or_else(|| "No pane text".to_string())?;
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            ss.get_screen_text(&pty_id)
                .unwrap_or_default()
                .replace('\n', "\r\n")
        };
        Ok(serde_json::json!({ "text": text, "formatted": formatted }))
    }
}

impl ServiceContext {
    /// If `pane_id` (or its alias) is a chatBackend pane, return its
    /// `(plugin_id, session_id)`; otherwise `None`. The guard the PTY entry
    /// methods use to short-circuit to the plugin transport before touching a
    /// PTY.
    pub(crate) fn pane_chat_backing(&self, pane_id: &str) -> Option<(String, String)> {
        let mux = self.mux.lock();
        let resolved = mux.resolve_pane_id(pane_id)?;
        match mux.get_pane(&resolved)?.backing.clone() {
            PaneBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            PaneBacking::Pty => None,
        }
    }

    /// Send-time outbound stage hook (plan Track B↔C contract). Runs the
    /// registered `chatPreprocessor` text stages over the outbound text before
    /// dispatching to transport. No stages ⇒ identity (Track B's shipped
    /// contract). Stages run in id order for determinism; a broken stage fails
    /// safe to passthrough (see [`JsChatPreprocessor`]).
    ///
    /// [`JsChatPreprocessor`]: crate::plugin::js_runtime::JsChatPreprocessor
    pub fn run_outbound_stages(&self, pane_id: &str, text: String) -> String {
        let mut stages = self.chat_preprocessor_registry.active();
        if stages.is_empty() {
            return text;
        }
        stages.sort_by(|a, b| a.id().cmp(b.id()));
        let ctx = crate::chatpreprocessor::PreprocessCtx {
            pane_id: pane_id.to_string(),
            ..Default::default()
        };
        crate::chatpipeline::run_text_stages(&stages, &ctx, text)
    }

    /// Resolve a pane (or alias) to its [`PaneTransport`]. Reads the pane's
    /// backing once and returns the matching impl. Errors if the pane is
    /// unknown.
    pub fn pane_transport(&self, pane_id: &str) -> Result<Box<dyn PaneTransport + '_>, String> {
        let (resolved, backing) = {
            let mux = self.mux.lock();
            let resolved = mux
                .resolve_pane_id(pane_id)
                .ok_or_else(|| format!("Pane {} not found", pane_id))?;
            let backing = mux
                .get_pane(&resolved)
                .map(|p| p.backing.clone())
                .ok_or_else(|| format!("Pane {} not found", pane_id))?;
            (resolved, backing)
        };
        match backing {
            PaneBacking::Pty => Ok(Box::new(PtyTransport {
                svc: self,
                pane_id: resolved,
            })),
            PaneBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Ok(Box::new(super::pane_transport_chat::ChatBackendTransport {
                svc: self,
                pane_id: resolved,
                plugin_id,
                session_id,
            })),
        }
    }
}
