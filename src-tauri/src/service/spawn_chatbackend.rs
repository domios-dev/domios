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
use crate::agent::chat_types::StructuredChat;
use crate::multiplexer::types::PaneBacking;

/// Adaptive inbound-poll cadence bounds. While a turn/stream is active (a
/// non-empty delta arrived on the previous tick) the loop polls at
/// [`POLL_ACTIVE_MS`] for near-token streaming; when idle it backs off
/// geometrically up to [`POLL_IDLE_MS`] so an open-but-quiet chat pane costs
/// almost nothing. The floor prevents a busy-spin.
const POLL_ACTIVE_MS: u64 = 60;
const POLL_IDLE_MS: u64 = 750;

/// Next sleep before the following poll. Active (`had_delta`) snaps back to the
/// fast cadence; idle doubles the current delay, clamped to `[ACTIVE, IDLE]`.
fn next_poll_delay_ms(had_delta: bool, current_ms: u64) -> u64 {
    if had_delta {
        POLL_ACTIVE_MS
    } else {
        current_ms
            .saturating_mul(2)
            .clamp(POLL_ACTIVE_MS, POLL_IDLE_MS)
    }
}

/// Byte-identity signature of a delta's emitted payload (`delta.messages` — the
/// exact bytes the sink would ship to the chat UI). A live streaming message
/// re-returned every tick with no growth (cursor pinned at `liveFrom`) hashes to
/// the same value, so the loop can suppress the redundant emit. Cheap u64 hash
/// over the serialized payload keeps per-tick cost O(n) without retaining the
/// payload between ticks.
fn delta_signature(delta: &StructuredChat) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&delta.messages)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

/// Per-tick poll-loop decision.
struct PollTick {
    /// Cursor to pass on the next poll: whatever the plugin returned, falling
    /// back to the previous cursor when it returned none.
    cursor: Option<String>,
    /// Emit the delta to the chat UI only when it carried NEW bytes (non-empty
    /// AND not byte-identical to the last emitted payload).
    emit: bool,
    /// Adaptive delay before the next poll.
    delay_ms: u64,
    /// Signature to remember for the next tick: the new payload's signature when
    /// a delta arrived, else the prior signature carried forward (so an idle gap
    /// between two identical deltas still suppresses the second).
    signature: Option<u64>,
}

/// Pure per-tick planning for the inbound poll loop: advance the cursor, decide
/// whether to emit, and pick the next adaptive delay. Emit only when the delta
/// is non-empty AND its payload differs from the last emitted one — a streaming
/// message re-returned unchanged (cursor pinned at `liveFrom`) is suppressed to
/// avoid redundant IPC + O(n²) re-transmission. Keeping this pure makes the
/// loop's contract unit-testable without a live daemon/sink.
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
            result
        };

        // Persisted as the JSONL `_meta` header so a pane restored after a
        // daemon restart can lazily re-open from the same systemPrompt/preset.
        let history_meta = config.clone();

        // Open the plugin session AFTER the pane exists so the SDK `openSession`
        // ctx carries the real `paneId` (a plugin may key its session on it,
        // e.g. LM Studio). On failure, tear down the just-created tab so a bad
        // config leaves no dangling empty pane.
        let session_id = match self.chatbackend_open_session(&plugin_id, &pane_id, config) {
            Ok(s) => s,
            Err(e) => {
                let _ = self.mux.write().close_tab(&ws_id, &tab_id);
                self.emit_state_changed();
                return Err(e);
            }
        };

        // Mark the backing — pty_id stays None permanently, no backend reader.
        {
            let mut mux = self.mux.write();
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
        // The sink also UPSERTs each delta into the session JSONL for durability.
        self.spawn_chatbackend_poll_loop(pane_id.clone(), plugin_id, session_id, history_meta);

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
            // Durable wrapper: forwards to the UI sink AND persists each delta to
            // the session JSONL (UPSERT by id — a streaming message rewrites its
            // own line, never duplicates).
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
            // Per-session cursor: the plugin returns only the DELTA after this
            // bookmark plus the next cursor. None on the first tick ⇒ the plugin
            // returns the whole transcript and seeds the cursor.
            let mut cursor: Option<String> = None;
            // Start idle; the first non-empty delta snaps the loop to the fast
            // cadence (adaptive: near-token streaming while active, ~zero when idle).
            let mut delay_ms = POLL_IDLE_MS;
            // Signature of the last emitted payload — a re-returned streaming
            // message with no growth hashes the same and is suppressed.
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
                    // Item 6: advance any async work (NPC agent tickets → replies)
                    // BEFORE reading the transcript, so a host-driven async
                    // chatBackend actually progresses on the real daemon.
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
        // A delta arrived this tick ⇒ poll fast next, regardless of prior delay.
        assert_eq!(next_poll_delay_ms(true, POLL_IDLE_MS), POLL_ACTIVE_MS);
        assert_eq!(next_poll_delay_ms(true, POLL_ACTIVE_MS), POLL_ACTIVE_MS);
        const { assert!(POLL_ACTIVE_MS <= 80, "active cadence must be near-token") };
    }

    #[test]
    fn cadence_backs_off_to_idle_when_quiet() {
        // Sustained idle climbs geometrically and converges to the idle cap,
        // never below the floor and never above the cap (no busy-spin).
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
        // Stays pinned at the idle cap once reached.
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
        // Empty delta with no fresh cursor ⇒ no emit, prior cursor retained,
        // cadence backs off.
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
        // First tick: prev cursor None, plugin returns whole transcript + cursor.
        let tick = plan_poll_tick(&delta(3, Some("c0")), None, POLL_IDLE_MS, None);
        assert!(tick.emit);
        assert_eq!(tick.cursor.as_deref(), Some("c0"));
    }

    #[test]
    fn tick_suppresses_byte_identical_delta() {
        // MED-2: a live streaming message re-returned with no growth (cursor
        // pinned at liveFrom) hashes identically and must NOT re-emit — redundant
        // IPC + O(n²) bytes otherwise.
        let d = delta(1, Some("liveFrom"));
        let sig = delta_signature(&d);
        // First sight: no prior signature → emit, remember the signature.
        let t1 = plan_poll_tick(&d, Some("c0".into()), POLL_ACTIVE_MS, None);
        assert!(t1.emit, "first delta must emit");
        assert_eq!(t1.signature, Some(sig));
        // Same delta again, prior signature matches → suppressed.
        let t2 = plan_poll_tick(&d, t1.cursor.clone(), t1.delay_ms, t1.signature);
        assert!(!t2.emit, "byte-identical re-returned delta must NOT emit");
        assert_eq!(t2.signature, Some(sig), "signature unchanged");
        assert_eq!(t2.delay_ms, POLL_ACTIVE_MS, "still active while streaming");
    }

    #[test]
    fn tick_emits_when_delta_grows() {
        // A streaming message that GREW (new tokens) changes its signature and
        // must emit.
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
        // An idle gap between two identical deltas must still suppress the second:
        // the prior signature is carried across the empty tick.
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
