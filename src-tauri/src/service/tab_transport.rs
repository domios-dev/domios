//! `TabTransport` — the single tab-I/O seam. Tab-keyed entry ops (send / read
//! / resize / kill) dispatch on [`TabBacking`] instead of calling `resolve_pty_id`
//! directly; [`PtyTransport`] keeps the PTY path verbatim, [`ChatBackendTransport`]
//! routes to the plugin registry. A CI grep guard keeps new `resolve_pty_id` calls out.

use super::pty_io::SendBatchItem;
use super::ServiceContext;
use crate::multiplexer::types::{Tab, TabBacking};
use codeterm_types::pty::BootStage;

/// Transport-agnostic tab I/O. One impl per [`TabBacking`].
pub trait TabTransport {
    /// Send user text to the tab (PTY paste, or plugin `sendMessage`).
    fn send(&self, text: &str, enter: bool) -> Result<(), String>;
    /// Send a delayed/scripted batch.
    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String>;
    /// Resize the underlying terminal. No-op for non-PTY transports.
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String>;
    /// Kill the backing process/session without removing the tab.
    fn kill(&self) -> Result<(), String>;
    /// Internal raw output transport — buffered PTY output or chatBackend
    /// transcript. Returns `{ output, offset }`.
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
    pub(crate) tab_id: String,
}

impl TabTransport for PtyTransport<'_> {
    fn send(&self, text: &str, enter: bool) -> Result<(), String> {
        if enter {
            self.svc.send_text(&self.tab_id, text)
        } else {
            self.svc.insert_text(&self.tab_id, text)
        }
    }

    fn send_batch(&self, batch: Vec<SendBatchItem>) -> Result<(), String> {
        self.svc.send_text_batch(&self.tab_id, batch)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.svc.resize_tab(&self.tab_id, cols, rows)
    }

    fn kill(&self) -> Result<(), String> {
        self.svc.kill_tab_pty(&self.tab_id)
    }

    fn read_output(
        &self,
        offset: Option<usize>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
        full: bool,
    ) -> Result<serde_json::Value, String> {
        let pty_id = self.svc.resolve_pty_id(&self.tab_id)?;
        let text = {
            let buf = self.svc.output_buf.lock();
            if full {
                buf.get_full_text(&pty_id).unwrap_or_default()
            } else {
                buf.get_text(&pty_id).unwrap_or_default()
            }
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
            // Re-emulate at the pty's REAL width: shell partial-line redraws
            // (zsh's % padding) erase the last output line at any other width.
            let cols = self
                .svc
                .pty
                .lock()
                .size(&pty_id)
                .map(|(c, _)| c)
                .unwrap_or(200);
            crate::pty::ansi::clean_terminal_output_sized(truncated, 500, cols)
        } else {
            truncated.to_string()
        };
        Ok(serde_json::json!({ "output": output, "offset": text.len() }))
    }

    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String> {
        let pty_id = self.svc.resolve_pty_id(&self.tab_id)?;
        let ss = self.svc.screen_state.lock();
        let text = if formatted {
            let bytes = ss
                .get_screen_formatted(&pty_id)
                .ok_or_else(|| "No Tab text".to_string())?;
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
    /// `(plugin_id, session_id)` if `tab_id` is a chatBackend tab, else `None` —
    /// the guard PTY entry methods use to short-circuit to the plugin transport.
    pub(crate) fn tab_chat_backing(&self, tab_id: &str) -> Option<(String, String)> {
        let mux = self.mux.write();
        let resolved = mux.resolve_tab_id(tab_id)?;
        match mux.get_tab(&resolved)?.backing.clone() {
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Some((plugin_id, session_id)),
            TabBacking::Pty => None,
        }
    }

    /// Whether `tab_id` is ready to consume agent input.
    /// Half-launched tabs may have `agent_type` before the provider TUI exists.
    pub(crate) fn tab_has_agent(&self, tab_id: &str) -> bool {
        let mux = self.mux.write();
        mux.resolve_tab_id(tab_id)
            .and_then(|resolved| mux.get_tab(&resolved).map(tab_accepts_agent_input))
            .unwrap_or(false)
    }

    /// Whether a tab is assigned to an agent, regardless of launch readiness.
    pub(crate) fn tab_has_agent_identity(&self, tab_id: &str) -> bool {
        let mux = self.mux.write();
        mux.resolve_tab_id(tab_id)
            .and_then(|resolved| mux.get_tab(&resolved).map(tab_has_agent_identity))
            .unwrap_or(false)
    }

    /// Run registered `chatPreprocessor` text stages over the outbound text (id order
    /// for determinism; no stages ⇒ identity; a broken stage fails safe to passthrough).
    pub fn run_outbound_stages(&self, tab_id: &str, text: String) -> String {
        let mut stages = self.chat_preprocessor_registry.active();
        if stages.is_empty() {
            return text;
        }
        stages.sort_by(|a, b| a.id().cmp(b.id()));
        let (cwd, provider) = {
            let mux = self.mux.read();
            let resolved = mux.resolve_tab_id(tab_id);
            let tab = resolved.as_deref().and_then(|id| mux.get_tab(id));
            (
                tab.and_then(|p| p.cwd.clone()),
                tab.and_then(|p| p.agent_type.clone()),
            )
        };
        let ctx = crate::chatpreprocessor::PreprocessCtx {
            tab_id: tab_id.to_string(),
            cwd,
            provider,
            ..Default::default()
        };
        crate::chatpipeline::run_text_stages(&stages, &ctx, text)
    }

    /// Resolve a tab to its [`TabTransport`] by reading its backing once. Errors if unknown.
    pub fn tab_transport(&self, tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        let (resolved, backing) = {
            let mux = self.mux.write();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| format!("Tab {} not found", tab_id))?;
            let backing = mux
                .get_tab(&resolved)
                .map(|p| p.backing.clone())
                .ok_or_else(|| format!("Tab {} not found", tab_id))?;
            (resolved, backing)
        };
        match backing {
            TabBacking::Pty => Ok(Box::new(PtyTransport {
                svc: self,
                tab_id: resolved,
            })),
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => Ok(Box::new(super::tab_transport_chat::ChatBackendTransport {
                svc: self,
                tab_id: resolved,
                plugin_id,
                session_id,
            })),
        }
    }
}

fn tab_accepts_agent_input(tab: &Tab) -> bool {
    tab.agent_type.is_some()
        && matches!(
            tab.boot_stage,
            Some(BootStage::AgentReady | BootStage::PtyReattached)
        )
}

fn tab_has_agent_identity(tab: &Tab) -> bool {
    tab.agent_type.is_some()
}

#[cfg(test)]
mod tests {
    use super::{next_char_boundary, tab_accepts_agent_input, tab_has_agent_identity};
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_types::pty::BootStage;

    #[test]
    fn next_char_boundary_handles_multibyte_offsets() {
        let s = "abкd";
        assert_eq!(next_char_boundary(s, 3), 4);
        assert_eq!(&s[next_char_boundary(s, 3)..], "d");
    }

    #[test]
    fn tab_agent_input_accepts_spawned_and_reattached_ready_stages() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).expect("initial Tab");
        tab.agent_type = Some("codex".into());
        tab.boot_stage = Some(BootStage::ShellReady);
        assert!(!tab_accepts_agent_input(&tab));

        tab.boot_stage = Some(BootStage::AgentReady);
        assert!(tab_accepts_agent_input(&tab));

        tab.boot_stage = Some(BootStage::PtyReattached);
        assert!(tab_accepts_agent_input(&tab));
    }

    #[test]
    fn agent_identity_is_present_before_agent_ready() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).expect("initial Tab");
        tab.agent_type = Some("codex".into());
        tab.boot_stage = Some(BootStage::ShellReady);

        assert!(!tab_accepts_agent_input(tab));
        assert!(tab_has_agent_identity(tab));
    }
}
