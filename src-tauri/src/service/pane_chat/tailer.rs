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

/// Sink for raw JSONL text deltas. The chat UI uses this — frontend
/// parses with the TS-side `parseJsonlSession`. The parallel
/// [`DeltaSink`] stays for any future Rust-side consumer.
pub trait RawTextSink: Send + Sync {
    fn emit(&self, pane_id: &str, text: String);
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

    /// True while the polling thread is still running. A tailer can die on its
    /// own (session file vanished for >10 polls, provider path went None) yet
    /// stay registered — the registry uses this to detect such a corpse and
    /// respawn a fresh tailer instead of handing back a dead stream.
    pub fn is_alive(&self) -> bool {
        self.join
            .lock()
            .as_ref()
            .map(|j| !j.is_finished())
            .unwrap_or(false)
    }

    /// Signal the worker thread to exit and join. Idempotent. Drops
    /// the handle by consumption.
    pub fn shutdown(self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(j) = self.join.lock().take() {
            let _ = j.join();
        }
    }

    /// Construct a handle from its parts. Used by the raw-text tailer
    /// which lives outside this file's primary `spawn_tailer` builder.
    pub fn new(
        stop_flag: Arc<AtomicBool>,
        offset: Arc<AtomicU64>,
        join: thread::JoinHandle<()>,
    ) -> Self {
        Self {
            stop_flag,
            offset,
            join: Mutex::new(Some(join)),
        }
    }
}

impl Drop for TailerHandle {
    /// A dropped handle MUST stop its polling thread. `ChatRegistry::insert`
    /// drops the losing handle on an idempotent skip, and remount churn
    /// (StrictMode / HMR) can race two `chat_start`s; without this the loser's
    /// thread keeps polling and emitting deltas, so every chat message is
    /// appended once per orphaned tailer — the "same bubble repeated N times"
    /// bug. Setting the flag (no join) lets the thread exit on its next poll.
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
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
        let mut cached_path: Option<PathBuf> = None;
        while !stop.load(Ordering::Acquire) {
            // Re-resolve the JSONL path each iteration. Cheap (plugin call + string concat) and lets the plugin transparently handle session-file
            // rotation. The `cached_path` is only for logging — we always re-ask the plugin.
            let path: Option<PathBuf> = registry
                .with_provider(&cfg.provider_id, |p| {
                    p.session_jsonl_path(&cfg.cwd, &cfg.session_id)
                })
                .flatten();
            let path = match path {
                Some(p) => p,
                None => {
                    if cached_path.is_some() {
                        log::warn!(
                            "[pane_chat] pane={} provider={} sid={} previously had a path but now returns None — stopping",
                            cfg.pane_id, cfg.provider_id, cfg.session_id
                        );
                    } else {
                        log::warn!(
                            "[pane_chat] no JSONL path for pane={} provider={} sid={}",
                            cfg.pane_id,
                            cfg.provider_id,
                            cfg.session_id
                        );
                    }
                    return;
                }
            };
            if cached_path.as_ref() != Some(&path) {
                log::info!(
                    "[pane_chat] pane={} tailing {}",
                    cfg.pane_id,
                    path.display()
                );
                cached_path = Some(path.clone());
                // TODO: future improvement — also re-fetch pane.agent_session_id from mux so an opencode model-swap that mints a new session retargets the
                // tailer. Today the session_id is fixed at spawn.
            }
            if !path.exists() {
                // The session JSONL may not exist YET: a fresh agent creates it
                // only on its first completed turn (seconds out on a slow first
                // turn / model spin-up). Keep watching — never permanently give
                // up — so the chat populates the moment the file appears. The
                // registry's stop flag (chat_stop / handle drop) bounds this
                // thread's lifetime, so a session that never materializes does
                // not spin forever. (Was: gave up after 10 missing polls → "No
                // messages yet" forever even after claude wrote the file.)
                thread::sleep(cfg.poll_interval);
                continue;
            }
            let from = off.load(Ordering::Acquire);
            // I4: parse_session_delta is fault-tolerant — it advances offset past chunks the plugin couldn't handle, so we never busy-loop on broken bytes.
            let delta = registry
                .with_provider(&cfg.provider_id, |p| {
                    let mut delta = p.parse_session_delta(&path, from);
                    for m in &mut delta.messages {
                        crate::service::pane_chat::enrich_interactive(m, p);
                    }
                    delta
                })
                .unwrap_or_else(|| crate::agent::chat_types::SessionDelta {
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

// Raw-text tailer emits byte chunks only; JS parses JSONL via `parseJsonlSession`.

pub struct RawTailConfig {
    pub pane_id: String,
    pub path: std::path::PathBuf,
    pub start_offset: u64,
    pub poll_interval: Duration,
}

pub fn spawn_raw_tailer(cfg: RawTailConfig, sink: Arc<dyn RawTextSink>) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let offset = Arc::new(AtomicU64::new(cfg.start_offset));

    let stop = stop_flag.clone();
    let off = offset.clone();
    let join = thread::spawn(move || {
        loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            // A fresh session's JSONL lags the session id (first write lands
            // with the first reply) — keep waiting; the registry's stop flag
            // bounds this thread's lifetime.
            if !cfg.path.exists() {
                thread::sleep(cfg.poll_interval);
                continue;
            }
            let from = off.load(Ordering::Acquire);
            let bytes = match std::fs::read(&cfg.path) {
                Ok(b) => b,
                Err(_) => {
                    thread::sleep(cfg.poll_interval);
                    continue;
                }
            };
            let total = bytes.len() as u64;
            if total > from {
                let slice = &bytes[from as usize..];
                let last_nl = slice.iter().rposition(|&b| b == b'\n');
                if let Some(idx) = last_nl {
                    let consumed = (idx + 1) as u64;
                    let text = String::from_utf8_lossy(&slice[..consumed as usize]).to_string();
                    off.store(from + consumed, Ordering::Release);
                    sink.emit(&cfg.pane_id, text);
                }
            } else if total < from {
                // File rotated/truncated. Reset to beginning so the next poll re-streams from byte 0 rather than spinning past EOF.
                log::info!(
                    "[pane_chat.raw] file shrank ({} → {}); resetting offset",
                    from,
                    total
                );
                off.store(0, Ordering::Release);
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    TailerHandle::new(stop_flag, offset, join)
}
