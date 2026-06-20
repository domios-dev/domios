//! Spawn path for non-PTY `chatBackend` panes (`pty_id: None`, no backend
//! reader); inbound transcript arrives via the plugin `poll` → `StructuredSink`.

use std::future::Future;

use super::spawn::{PaneCommand, PaneHandle, SpawnPaneSpec};
use super::ServiceContext;
use crate::agent::chat_types::StructuredChat;
use crate::multiplexer::types::PaneBacking;

/// Adaptive inbound-poll cadence bounds: fast while streaming, geometric
/// back-off to idle so an open-but-quiet chat pane costs almost nothing.
const POLL_ACTIVE_MS: u64 = 60;
const POLL_IDLE_MS: u64 = 750;

fn next_poll_delay_ms(had_delta: bool, current_ms: u64) -> u64 {
    if had_delta {
        POLL_ACTIVE_MS
    } else {
        current_ms
            .saturating_mul(2)
            .clamp(POLL_ACTIVE_MS, POLL_IDLE_MS)
    }
}

/// Byte-identity signature of a delta's payload, so a streaming message
/// re-returned unchanged hashes the same and the redundant emit is suppressed.
fn delta_signature(delta: &StructuredChat) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&delta.messages)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

struct PollTick {
    cursor: Option<String>,
    emit: bool,
    delay_ms: u64,
    signature: Option<u64>,
}

/// Pure per-tick planning for the poll loop. Emit only when the delta is
/// non-empty AND differs from the last emitted payload — a streaming message
/// re-returned unchanged is suppressed to avoid redundant IPC + O(n²) bytes.
fn plan_poll_tick(
    delta: &StructuredChat,
    prev_cursor: Option<String>,
    prev_delay: u64,
    prev_signature: Option<u64>,
) -> PollTick {
    let had_delta = !delta.messages.is_empty();
    let signature = if had_delta {
        Some(delta_signature(delta))
    } else {
        prev_signature
    };
    PollTick {
        cursor: delta.cursor.clone().or(prev_cursor),
        emit: had_delta && signature != prev_signature,
        delay_ms: next_poll_delay_ms(had_delta, prev_delay),
        signature,
    }
}

/// Spawn on the daemon's global tauri runtime. The synchronous socket-RPC spawn
/// path has no ambient Tokio runtime, where bare `tokio::spawn` would panic.
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
            let mux = self.mux.write();
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
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_pane(&result.1, title) {
                    log::warn!("[spawn.chatbackend] rename_pane failed: {}", e);
                }
                let _ = mux.rename_tab(&ws_id, &result.0, title);
            }
            if let Some(ref description) = spec.description {
                let _ = mux.set_pane_description(&result.1, Some(description.clone()));
            }
            result
        };

        // Persisted as the JSONL `_meta` header so a restored pane can re-open from the same config.
        let history_meta = config.clone();

        // Pane shows instantly with an empty session id; openSession runs in the background.
        {
            let mut mux = self.mux.write();
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                pane.backing = PaneBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: String::new(),
                };
                pane.chat_view = true;
                if let Some(ref dir) = resolved_cwd {
                    pane.cwd = Some(dir.clone());
                }
            }
        }
        self.emit_state_changed();

        self.chatbackend_launch_async(
            plugin_id,
            pane_id.clone(),
            tab_id.clone(),
            ws_id.clone(),
            config,
            history_meta,
        );

        Ok(PaneHandle {
            pane_id,
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            description: spec.description,
            worker_id: None,
            warning: None,
        })
    }

    /// Background launch: open the plugin session, stamp its id onto the backing,
    /// publish models, start the poll loop. On openSession failure the tab is
    /// torn down so a bad config leaves no dangling empty pane.
    pub(crate) fn chatbackend_launch_async(
        &self,
        plugin_id: String,
        pane_id: String,
        tab_id: String,
        ws_id: String,
        config: serde_json::Value,
        history_meta: serde_json::Value,
    ) {
        let svc = self.clone();
        spawn_bg(async move {
            let (open_svc, open_plugin, open_pane, open_cfg) =
                (svc.clone(), plugin_id.clone(), pane_id.clone(), config);
            let session_id = match tokio::task::spawn_blocking(move || {
                open_svc.chatbackend_open_session(&open_plugin, &open_pane, open_cfg)
            })
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    log::warn!(
                        "[spawn.chatbackend] openSession failed for {}: {}",
                        pane_id,
                        e
                    );
                    let _ = svc.mux.write().close_tab(&ws_id, &tab_id);
                    svc.emit_state_changed();
                    svc.create_notification(
                        format!("Failed to open {} session", plugin_id),
                        Some(e),
                        Some("error"),
                    );
                    return;
                }
                Err(e) => {
                    log::warn!("[spawn.chatbackend] openSession task join error: {}", e);
                    let _ = svc.mux.write().close_tab(&ws_id, &tab_id);
                    svc.emit_state_changed();
                    return;
                }
            };

            {
                let mut mux = svc.mux.write();
                if let Some(pane) = mux.get_pane_mut(&pane_id) {
                    if let PaneBacking::ChatBackend {
                        session_id: sid, ..
                    } = &mut pane.backing
                    {
                        *sid = session_id.clone();
                    }
                }
            }
            svc.emit_state_changed();

            log::info!(
                "[spawn.chatbackend] pane {} backed by plugin '{}' session '{}'",
                pane_id,
                plugin_id,
                session_id
            );

            // Publish models for the pane picker; listModels may hit a network backend, so spawn_blocking.
            {
                let svc2 = svc.clone();
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

            svc.spawn_chatbackend_poll_loop(pane_id, plugin_id, session_id, history_meta);
        });
    }

    /// Background loop: poll the plugin and emit new transcript on
    /// `pane-chat-msg-{id}`, ending when the pane closes. Poll runs on a blocking
    /// thread so a slow plugin can't stall the async runtime.
    pub(crate) fn spawn_chatbackend_poll_loop(
        &self,
        pane_id: String,
        plugin_id: String,
        session_id: String,
        history_meta: serde_json::Value,
    ) {
        let svc = self.clone();
        let app = self.app_handle.clone();
        spawn_bg(async move {
            let inner = crate::service::pane_chat::structured_sink::make_sink(app);
            // Durable wrapper: forwards to the UI sink and UPSERTs each delta to the session JSONL by id.
            let sink: std::sync::Arc<
                dyn crate::service::pane_chat::structured_sink::StructuredSink,
            > = std::sync::Arc::new(
                crate::service::pane_chat::structured_sink::ChatBackendStructuredSink::new(
                    inner,
                    &plugin_id,
                    &session_id,
                    Some(&history_meta),
                ),
            );
            // None on the first tick ⇒ the plugin returns the whole transcript and seeds the cursor.
            let mut cursor: Option<String> = None;
            let mut delay_ms = POLL_IDLE_MS;
            let mut last_signature: Option<u64> = None;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                if svc.mux.read().get_pane(&pane_id).is_none() {
                    break;
                }
                let (svc2, plugin2, sid2, cur) = (
                    svc.clone(),
                    plugin_id.clone(),
                    session_id.clone(),
                    cursor.clone(),
                );
                match tokio::task::spawn_blocking(move || {
                    // Advance async work before reading, so a host-driven async chatBackend progresses.
                    let _ = svc2.chatbackend_pump(&plugin2, &sid2);
                    svc2.chatbackend_poll(&plugin2, &sid2, cur.as_deref())
                })
                .await
                {
                    Ok(Ok(delta)) => {
                        let tick = plan_poll_tick(&delta, cursor.take(), delay_ms, last_signature);
                        cursor = tick.cursor;
                        delay_ms = tick.delay_ms;
                        last_signature = tick.signature;
                        if tick.emit {
                            sink.emit(&pane_id, delta.messages, None);
                        }
                    }
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

    use super::{
        delta_signature, next_poll_delay_ms, plan_poll_tick, POLL_ACTIVE_MS, POLL_IDLE_MS,
    };
    use crate::agent::chat_types::{ChatMsg, StructuredChat};

    fn delta(n: usize, cursor: Option<&str>) -> StructuredChat {
        StructuredChat {
            messages: (0..n)
                .map(|i| ChatMsg {
                    id: i.to_string(),
                    ..Default::default()
                })
                .collect(),
            cursor: cursor.map(|c| c.to_string()),
        }
    }

    #[test]
    fn cadence_is_fast_while_active() {
        assert_eq!(next_poll_delay_ms(true, POLL_IDLE_MS), POLL_ACTIVE_MS);
        assert_eq!(next_poll_delay_ms(true, POLL_ACTIVE_MS), POLL_ACTIVE_MS);
        const { assert!(POLL_ACTIVE_MS <= 80, "active cadence must be near-token") };
    }

    #[test]
    fn cadence_backs_off_to_idle_when_quiet() {
        let mut d = POLL_ACTIVE_MS;
        let mut steps = 0;
        loop {
            let next = next_poll_delay_ms(false, d);
            assert!((POLL_ACTIVE_MS..=POLL_IDLE_MS).contains(&next));
            assert!(next > d || next == POLL_IDLE_MS, "must grow until capped");
            d = next;
            steps += 1;
            if d == POLL_IDLE_MS {
                break;
            }
            assert!(steps < 20, "must converge to idle cap");
        }
        assert_eq!(next_poll_delay_ms(false, POLL_IDLE_MS), POLL_IDLE_MS);
    }

    #[test]
    fn tick_emits_and_advances_cursor_on_nonempty_delta() {
        let tick = plan_poll_tick(&delta(2, Some("c2")), Some("c1".into()), POLL_IDLE_MS, None);
        assert!(tick.emit, "non-empty delta must emit");
        assert_eq!(tick.cursor.as_deref(), Some("c2"), "cursor advances");
        assert_eq!(tick.delay_ms, POLL_ACTIVE_MS, "active ⇒ fast next poll");
    }

    #[test]
    fn tick_idle_does_not_emit_and_keeps_cursor() {
        let tick = plan_poll_tick(&delta(0, None), Some("c1".into()), POLL_ACTIVE_MS, None);
        assert!(!tick.emit, "empty delta must NOT emit");
        assert_eq!(
            tick.cursor.as_deref(),
            Some("c1"),
            "cursor unchanged when idle"
        );
        assert!(tick.delay_ms > POLL_ACTIVE_MS, "idle backs off");
    }

    #[test]
    fn tick_first_poll_no_cursor_emits_full_transcript() {
        let tick = plan_poll_tick(&delta(3, Some("c0")), None, POLL_IDLE_MS, None);
        assert!(tick.emit);
        assert_eq!(tick.cursor.as_deref(), Some("c0"));
    }

    #[test]
    fn tick_suppresses_byte_identical_delta() {
        let d = delta(1, Some("liveFrom"));
        let sig = delta_signature(&d);
        let t1 = plan_poll_tick(&d, Some("c0".into()), POLL_ACTIVE_MS, None);
        assert!(t1.emit, "first delta must emit");
        assert_eq!(t1.signature, Some(sig));
        let t2 = plan_poll_tick(&d, t1.cursor.clone(), t1.delay_ms, t1.signature);
        assert!(!t2.emit, "byte-identical re-returned delta must NOT emit");
        assert_eq!(t2.signature, Some(sig), "signature unchanged");
        assert_eq!(t2.delay_ms, POLL_ACTIVE_MS, "still active while streaming");
    }

    #[test]
    fn tick_emits_when_delta_grows() {
        let d1 = delta(1, Some("c1"));
        let t1 = plan_poll_tick(&d1, None, POLL_IDLE_MS, None);
        assert!(t1.emit);
        let d2 = delta(2, Some("c2"));
        let t2 = plan_poll_tick(&d2, t1.cursor.clone(), t1.delay_ms, t1.signature);
        assert!(t2.emit, "grown delta must emit");
        assert_ne!(t2.signature, t1.signature, "signature reflects new content");
    }

    #[test]
    fn tick_idle_preserves_last_signature_for_later_dedup() {
        // The prior signature must survive an empty tick so a later identical delta stays suppressed.
        let d = delta(1, Some("c1"));
        let t1 = plan_poll_tick(&d, None, POLL_ACTIVE_MS, None);
        assert!(t1.emit);
        let idle = plan_poll_tick(
            &delta(0, None),
            t1.cursor.clone(),
            t1.delay_ms,
            t1.signature,
        );
        assert!(!idle.emit, "empty delta does not emit");
        assert_eq!(idle.signature, t1.signature, "idle keeps prior signature");
        let again = plan_poll_tick(&d, idle.cursor.clone(), idle.delay_ms, idle.signature);
        assert!(
            !again.emit,
            "same content after an idle gap must stay suppressed"
        );
    }

    /// Repro for the socket-RPC spawn panic: a plain `#[test]` (no `#[tokio::test]`)
    /// reproduces the no-ambient-runtime thread where bare `tokio::spawn` panics
    /// but `spawn_bg` (daemon's global tauri runtime) still runs the future.
    #[test]
    fn poll_loop_spawn_does_not_need_ambient_runtime() {
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "test must reproduce the no-ambient-runtime context"
        );

        // Suppress the panic hook so the expected catch_unwind panic doesn't pollute output.
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

        let (tx, rx) = mpsc::channel();
        super::spawn_bg(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("poll loop future must run on the tauri runtime, not the ambient one");
    }
}
