//! Spawn path for `chatBackend` panes (plan Track B, Appendix B1.3 #6).
//!
//! A chatBackend pane is the non-PTY pane: it registers with `pty_id: None` and
//! `backing: PaneBacking::ChatBackend`, opens a plugin session, and starts NO
//! backend reader (so `process_pty_output`/`backend_read_loop`/`ScreenState` are
//! never entered for it — B1.1#3). Inbound messages arrive via the plugin
//! `poll` → `StructuredSink` path, not a PTY tail.

use std::future::Future;

use super::spawn::{PaneCommand, PaneHandle, SpawnPaneSpec};
use super::ServiceContext;
use crate::multiplexer::types::PaneBacking;

/// How often the inbound poll loop pulls new transcript from the plugin.
const POLL_INTERVAL_MS: u64 = 500;

/// Spawn chatBackend background work on the daemon's main Tokio runtime.
///
/// chatBackend panes are spawned from three call sites: the async HTTP route,
/// the Tauri UI command, and the **synchronous socket RPC** (`spawn_agent_sync`,
/// which runs on a plain blocking thread with NO ambient Tokio runtime). Bare
/// `tokio::spawn` needs a runtime in thread-local context and panics ("there is
/// no reactor running, must be called from the context of a Tokio 1.x runtime")
/// on that last path. `tauri::async_runtime` holds a global handle to the
/// daemon's runtime and spawns regardless of the caller's context.
fn spawn_bg<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tauri::async_runtime::spawn(fut);
}

impl ServiceContext {
    pub(crate) fn spawn_chatbackend_impl(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        let (plugin_id, config) = match &spec.command {
            PaneCommand::ChatBackend { plugin_id, config } => (plugin_id.clone(), config.clone()),
            _ => return Err("spawn_chatbackend_impl called without ChatBackend command".into()),
        };

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

        // Open the plugin session AFTER the pane exists so the SDK `openSession`
        // ctx carries the real `paneId` (a plugin may key its session on it,
        // e.g. LM Studio). On failure, tear down the just-created tab so a bad
        // config leaves no dangling empty pane.
        let session_id = match self.chatbackend_open_session(&plugin_id, &pane_id, config) {
            Ok(s) => s,
            Err(e) => {
                let _ = self.mux.lock().close_tab(&ws_id, &tab_id);
                self.emit_state_changed();
                return Err(e);
            }
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

        // Best-effort: publish the backend's models into the catalog so the
        // pane model picker can resolve them. Off-thread — listModels may hit a
        // network backend (e.g. LM Studio) and must never block the spawn.
        {
            let svc2 = self.clone();
            let plugin2 = plugin_id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Ok(models) = svc2.chatbackend_list_models(&plugin2) {
                    if !models.is_empty() {
                        crate::agent::model_catalog::register(
                            &plugin2,
                            crate::agent::model_catalog::RegisteredDefaults {
                                models,
                                default: None,
                            },
                        );
                    }
                }
            });
        }

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
        spawn_bg(async move {
            let sink = crate::service::pane_chat::structured_sink::make_sink(app);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                if svc.mux.lock().get_pane(&pane_id).is_none() {
                    break;
                }
                let (svc2, plugin2, sid2) = (svc.clone(), plugin_id.clone(), session_id.clone());
                match tokio::task::spawn_blocking(move || {
                    // Item 6: advance any async work (NPC agent tickets → replies)
                    // BEFORE reading the transcript, so a host-driven async
                    // chatBackend actually progresses on the real daemon.
                    let _ = svc2.chatbackend_pump(&plugin2, &sid2);
                    svc2.chatbackend_poll(&plugin2, &sid2)
                })
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    /// Repro for the socket-RPC `agent.spawn <chatBackend>` panic. The poll loop
    /// used `tokio::spawn`, which requires the caller to already be inside a
    /// Tokio runtime. The synchronous socket RPC runs the spawn on a plain
    /// blocking thread with NO ambient runtime, so it panicked with "there is no
    /// reactor running, must be called from the context of a Tokio 1.x runtime".
    /// `spawn_bg` routes onto the daemon's tauri runtime (global handle), which
    /// works regardless of caller context. A plain `#[test]` (NOT
    /// `#[tokio::test]`) reproduces that no-runtime thread.
    #[test]
    fn poll_loop_spawn_does_not_need_ambient_runtime() {
        // This thread has no ambient runtime — exactly the socket-RPC situation.
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "test must reproduce the no-ambient-runtime context"
        );

        // The bug condition is real on this thread: a bare `tokio::spawn` panics
        // with "there is no reactor running". Suppress the panic hook so the
        // catch_unwind doesn't pollute test output.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let bare = std::panic::catch_unwind(|| {
            tokio::spawn(async {});
        });
        std::panic::set_hook(prev);
        assert!(
            bare.is_err(),
            "precondition: tokio::spawn must panic without an ambient runtime"
        );

        // The fix: `spawn_bg` uses the daemon's tauri runtime, so the future runs
        // even though THIS thread never entered a runtime.
        let (tx, rx) = mpsc::channel();
        super::spawn_bg(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("poll loop future must run on the tauri runtime, not the ambient one");
    }
}
