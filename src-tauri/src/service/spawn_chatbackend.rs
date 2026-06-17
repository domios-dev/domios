//! Spawn path for `chatBackend` panes (plan Track B, Appendix B1.3 #6).
//!
//! A chatBackend pane is the non-PTY pane: it registers with `pty_id: None` and
//! `backing: PaneBacking::ChatBackend`, opens a plugin session, and starts NO
//! backend reader (so `process_pty_output`/`backend_read_loop`/`ScreenState` are
//! never entered for it — B1.1#3). Inbound messages arrive via the plugin
//! `poll` → `StructuredSink` path, not a PTY tail.

use super::spawn::{PaneCommand, PaneHandle, SpawnPaneSpec};
use super::ServiceContext;
use crate::multiplexer::types::PaneBacking;

/// How often the inbound poll loop pulls new transcript from the plugin.
const POLL_INTERVAL_MS: u64 = 500;

impl ServiceContext {
    pub(crate) fn spawn_chatbackend_impl(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        let (plugin_id, config) = match &spec.command {
            PaneCommand::ChatBackend { plugin_id, config } => (plugin_id.clone(), config.clone()),
            _ => return Err("spawn_chatbackend_impl called without ChatBackend command".into()),
        };

        // Open the plugin session first — fail fast before creating a pane so a
        // bad config doesn't leave a dangling empty pane.
        let session_id = self.chatbackend_open_session(&plugin_id, config)?;

        let (ws_id, ws_cwd) = {
            let mux = self.mux.lock();
            let from_parent = spec
                .parent_pane_id
                .as_ref()
                .and_then(|pid| mux.find_workspace_for_pane(pid));
            let id = from_parent
                .or_else(|| spec.workspace_id.clone())
                .or_else(|| mux.snapshot().active_workspace_id.clone())
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or("No workspace found")?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let (tab_id, pane_id) = {
            let mut mux = self.mux.lock();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_pane(&result.1, title) {
                    log::warn!("[spawn.chatbackend] rename_pane failed: {}", e);
                }
                let _ = mux.rename_tab(&ws_id, &result.0, title);
            }
            result
        };

        // Mark the backing — pty_id stays None permanently, no backend reader.
        {
            let mut mux = self.mux.lock();
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.backing = PaneBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: session_id.clone(),
                };
                // Render as chat (the frontend also branches on `backing`).
                pane.chat_view = true;
                if let Some(ref dir) = resolved_cwd {
                    pane.cwd = Some(dir.clone());
                }
            }
        }
        self.emit_state_changed();

        log::info!(
            "[spawn.chatbackend] pane {} backed by plugin '{}' session '{}'",
            pane_id,
            plugin_id,
            session_id
        );

        // Inbound transcript arrives via poll → StructuredSink (no PTY reader).
        self.spawn_chatbackend_poll_loop(pane_id.clone(), plugin_id, session_id);

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            worker_id: None,
            warning: None,
        })
    }

    /// Background loop: poll the plugin for new transcript and emit it on the
    /// `pane-chat-msg-{id}` event the frontend already consumes. Ends when the
    /// pane is closed. The poll runs on a blocking thread so a slow plugin
    /// `poll` can't stall the async runtime (the VM lock is per-plugin).
    fn spawn_chatbackend_poll_loop(&self, pane_id: String, plugin_id: String, session_id: String) {
        let svc = self.clone();
        let app = self.app_handle.clone();
        tokio::spawn(async move {
            let sink = crate::service::pane_chat::structured_sink::make_sink(app);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                if svc.mux.lock().get_pane(&pane_id).is_none() {
                    break;
                }
                let (svc2, plugin2, sid2) = (svc.clone(), plugin_id.clone(), session_id.clone());
                match tokio::task::spawn_blocking(move || svc2.chatbackend_poll(&plugin2, &sid2))
                    .await
                {
                    Ok(Ok(msgs)) if !msgs.is_empty() => sink.emit(&pane_id, msgs, None),
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        log::warn!("[chatbackend] poll pane {} failed: {}", pane_id, e)
                    }
                    Err(e) => {
                        log::warn!("[chatbackend] poll task join error: {}", e);
                        break;
                    }
                }
            }
            log::info!("[chatbackend] poll loop ended for pane {}", pane_id);
        });
    }
}
