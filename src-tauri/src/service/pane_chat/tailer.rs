//! One-pane JSONL tailer. Owns a thread that polls the session file,
//! parses deltas via the agent provider plugin, and emits via the
//! supplied sink. Stops when [`TailerHandle::shutdown`] is called
//! or when the session file disappears for >10 consecutive polls
//! (provider closed the session — no point spinning).

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::agent::chat_types::RichMessage;
use crate::agent::registry::AgentRegistry;

/// Caller-supplied sink. Real impl emits a Tauri event with the
/// messages; tests pass a `Vec<RichMessage>` collector.
pub trait DeltaSink: Send + Sync {
    fn emit(&self, pane_id: &str, messages: Vec<RichMessage>);
}

pub struct TailerHandle {
    stop_flag: Arc<AtomicBool>,
    offset: Arc<AtomicU64>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl TailerHandle {
    /// Current byte offset into the session JSONL. Useful for backfill —
    /// the REST `chat/backfill` endpoint reads the range `[offset - N, offset)`.
    pub fn current_offset(&self) -> u64 {
        self.offset.load(Ordering::Acquire)
    }

    /// Signal the worker thread to exit and join. Idempotent. Drops
    /// the handle by consumption.
    pub fn shutdown(self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(j) = self.join.lock().take() {
            let _ = j.join();
        }
    }
}

pub struct TailConfig {
    pub pane_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    /// Byte offset to begin from. Pass the offset returned by the
    /// initial snapshot (`parse_session_delta` from `len - snapshot_window`
    /// to `len`) so the tailer only emits truly new content.
    pub start_offset: u64,
    pub poll_interval: Duration,
}

pub fn spawn_tailer(
    cfg: TailConfig,
    registry: Arc<AgentRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let offset = Arc::new(AtomicU64::new(cfg.start_offset));

    let stop = stop_flag.clone();
    let off = offset.clone();
    let join = thread::spawn(move || {
        let path: Option<PathBuf> = registry
            .with_provider(&cfg.provider_id, |p| {
                p.session_jsonl_path(&cfg.cwd, &cfg.session_id)
            })
            .flatten();
        let path = match path {
            Some(p) => p,
            None => {
                log::warn!(
                    "[pane_chat] no JSONL path for pane={} provider={} sid={}",
                    cfg.pane_id, cfg.provider_id, cfg.session_id
                );
                return;
            }
        };
        let mut missing_streak = 0u32;
        while !stop.load(Ordering::Acquire) {
            if !path.exists() {
                missing_streak += 1;
                if missing_streak > 10 {
                    log::info!(
                        "[pane_chat] giving up on {:?} (10 polls missing)",
                        path
                    );
                    return;
                }
                thread::sleep(cfg.poll_interval);
                continue;
            }
            missing_streak = 0;
            let from = off.load(Ordering::Acquire);
            let delta = registry
                .with_provider(&cfg.provider_id, |p| {
                    p.parse_session_delta(&path, from)
                })
                .unwrap_or(crate::agent::chat_types::SessionDelta {
                    new_offset: from,
                    messages: vec![],
                });
            if delta.new_offset != from {
                off.store(delta.new_offset, Ordering::Release);
            }
            if !delta.messages.is_empty() {
                sink.emit(&cfg.pane_id, delta.messages);
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    TailerHandle {
        stop_flag,
        offset,
        join: Mutex::new(Some(join)),
    }
}
