//! Spawn path for `chatBackend` panes (plan Track B, Appendix B1.3 #6).
//!
//! A chatBackend pane is the non-PTY pane: it registers with `pty_id: None` and
//! `backing: PaneBacking::ChatBackend`, opens a plugin session, and starts NO
//! backend reader (so `process_pty_output`/`backend_read_loop`/`ScreenState` are
//! never entered for it — B1.1#3). Inbound messages arrive via the plugin
//! `poll` → `StructuredSink` path, not a PTY tail.

use super::spawn::{PaneCommand, PaneHandle, SpawnPaneSpec};
use super::ServiceContext;

impl ServiceContext {
    pub(crate) fn spawn_chatbackend_impl(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        let (_plugin_id, _config) = match &spec.command {
            PaneCommand::ChatBackend { plugin_id, config } => (plugin_id.clone(), config.clone()),
            _ => return Err("spawn_chatbackend_impl called without ChatBackend command".into()),
        };
        // Filled in B1.3 #6 (spawn path: register pane with PaneBacking::ChatBackend,
        // openSession, no backend reader, inbound poll→StructuredSink).
        Err("chatBackend spawn not yet implemented".into())
    }
}
