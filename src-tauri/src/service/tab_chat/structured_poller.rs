//! Per-tab poller for DB-backed providers. Sibling of the JSONL tailer:
//! same lifecycle (started on chat_start, stopped via the shared
//! `TailerHandle`), but it polls the provider's `read_structured_chat`
//! instead of tailing a file, and emits already-parsed `ChatMsg`s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::agent::registry::AgentRegistry;

use super::structured_sink::StructuredSink;
use super::tailer::TailerHandle;

pub struct StructuredPollConfig {
    pub tab_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    pub start_cursor: Option<String>,
    pub poll_interval: Duration,
}

pub fn spawn_structured_poller(
    cfg: StructuredPollConfig,
    registry: Arc<AgentRegistry>,
    sink: Arc<dyn StructuredSink>,
) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let offset = Arc::new(AtomicU64::new(0));

    let stop = stop_flag.clone();
    let off = offset.clone();
    let join = thread::spawn(move || {
        log::info!(
            "[structured-poller] started tab={} provider={} sid={} cursor={:?}",
            cfg.tab_id,
            cfg.provider_id,
            cfg.session_id,
            cfg.start_cursor
        );
        let mut cursor = cfg.start_cursor.clone();
        let mut last_emit_cursor: Option<String> = None;
        while !stop.load(Ordering::Acquire) {
            let delta = registry
                .with_provider(&cfg.provider_id, |p| {
                    p.read_structured_chat(&cfg.cwd, &cfg.session_id, cursor.as_deref())
                })
                .unwrap_or_default();
            // boundary message every tick.
            if !delta.messages.is_empty() && delta.cursor != last_emit_cursor {
                log::debug!(
                    "[structured-poller] tab={} emit {} msg(s) cursor={:?}",
                    cfg.tab_id,
                    delta.messages.len(),
                    delta.cursor
                );
                last_emit_cursor = delta.cursor.clone();
                sink.emit(&cfg.tab_id, delta.messages, delta.cursor.clone());
            }
            if let Some(c) = delta.cursor {
                if let Ok(n) = c.parse::<u64>() {
                    off.store(n, Ordering::Release);
                }
                cursor = Some(c);
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    TailerHandle::new(stop_flag, offset, join)
}
