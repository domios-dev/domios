//! One-tab JSONL tailer. Owns a thread that polls the session file,
//! parses deltas via the agent provider plugin, and emits via the
//! supplied sink. Stops when [`TailerHandle::shutdown`] is called
//! or when a session file that was PREVIOUSLY OBSERVED TO EXIST goes
//! missing for >10 consecutive polls (provider rotated/closed the
//! session — no point spinning). A file that has never yet appeared
//! (agent hasn't completed its first turn) is never reaped this way —
//! see the W3-A regression (commit 05576a7d9): reaping on first-ever
//! absence left chat empty forever on a slow first turn.

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
    fn emit(&self, tab_id: &str, messages: Vec<RichMessage>);
}

/// Sink for raw JSONL text deltas. The chat UI uses this — frontend
/// parses with the TS-side `parseJsonlSession`. The parallel
/// [`DeltaSink`] stays for any future Rust-side consumer.
pub trait RawTextSink: Send + Sync {
    fn emit(&self, tab_id: &str, text: String);
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
    /// own (a previously-seen session file vanished for >10 consecutive polls,
    /// or provider path went None) yet stay registered — the registry uses
    /// this to detect such a corpse and respawn a fresh tailer instead of
    /// handing back a dead stream.
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
    pub tab_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    /// Byte offset to begin from. Pass the offset returned by the
    /// initial snapshot (`parse_session_delta` from `len - snapshot_window`
    /// to `len`) so the tailer only emits truly new content.
    pub start_offset: u64,
    pub poll_interval: Duration,
}

/// A session file that was previously seen to exist and then goes missing
/// for more than this many consecutive polls is treated as gone for good
/// (provider rotated/closed the session) — the tailer thread exits so
/// [`crate::service::tab_chat::ChatRegistry`] can detect the corpse via
/// [`TailerHandle::is_alive`] and spawn a fresh tailer on the next attach.
/// Does NOT apply before the file has ever been observed to exist — see
/// the module doc for the W3-A regression this must not reintroduce.
const MAX_CONSECUTIVE_MISSING_POLLS: u32 = 10;

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
        let mut ever_existed = false;
        let mut missing_polls: u32 = 0;
        while !stop.load(Ordering::Acquire) {
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
                            "[tab_chat] tab={} provider={} sid={} previously had a path but now returns None — stopping",
                            cfg.tab_id, cfg.provider_id, cfg.session_id
                        );
                    } else {
                        log::warn!(
                            "[tab_chat] no JSONL path for tab={} provider={} sid={}",
                            cfg.tab_id,
                            cfg.provider_id,
                            cfg.session_id
                        );
                    }
                    return;
                }
            };
            if cached_path.as_ref() != Some(&path) {
                log::info!(
                    "[tab_chat] tab={} tailing {}",
                    cfg.tab_id,
                    path.display()
                );
                cached_path = Some(path.clone());
                // tailer. Today the session_id is fixed at spawn.
            }
            if !path.exists() {
                if ever_existed {
                    missing_polls += 1;
                    if missing_polls > MAX_CONSECUTIVE_MISSING_POLLS {
                        log::warn!(
                            "[tab_chat] tab={} provider={} sid={} session file missing for {} consecutive polls — stopping",
                            cfg.tab_id, cfg.provider_id, cfg.session_id, missing_polls
                        );
                        return;
                    }
                }
                thread::sleep(cfg.poll_interval);
                continue;
            }
            ever_existed = true;
            missing_polls = 0;
            let from = off.load(Ordering::Acquire);
            // I4: parse_session_delta is fault-tolerant — it advances offset past chunks the plugin couldn't handle, so we never busy-loop on broken bytes.
            let delta = registry
                .with_provider(&cfg.provider_id, |p| {
                    let mut delta = p.parse_session_delta(&path, from);
                    for m in &mut delta.messages {
                        crate::service::tab_chat::enrich_interactive(m, p);
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
                sink.emit(&cfg.tab_id, delta.messages);
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
    pub tab_id: String,
    pub path: std::path::PathBuf,
    pub start_offset: u64,
    pub poll_interval: Duration,
}

/// Bytes one raw-tailer poll forwards; the rest lands on the next poll.
const RAW_TAIL_CHUNK_BYTES: u64 = 8 * 1024 * 1024;

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
            if !cfg.path.exists() {
                thread::sleep(cfg.poll_interval);
                continue;
            }
            let from = off.load(Ordering::Acquire);
            let total = match std::fs::metadata(&cfg.path) {
                Ok(m) => m.len(),
                Err(_) => {
                    thread::sleep(cfg.poll_interval);
                    continue;
                }
            };
            if total > from {
                // Bounded seek read: the file behind this offset can be hundreds of MB.
                let window_end = total.min(from.saturating_add(RAW_TAIL_CHUNK_BYTES));
                let Ok((slice, _)) =
                    codeterm_plugin::jsonl_window::read_range(&cfg.path, from, window_end)
                else {
                    thread::sleep(cfg.poll_interval);
                    continue;
                };
                let slice = slice.as_slice();
                // Bytes through the last newline are unambiguously complete lines.
                let nl_consumed = slice
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map(|idx| idx + 1)
                    .unwrap_or(0);
                let tail = &slice[nl_consumed..];
                let tail_end = tail.iter().rposition(|&b| !b.is_ascii_whitespace());
                // Only trust an unterminated tail as a whole record at EOF.
                let tail_complete = window_end == total
                    && tail_end
                        .map(|e| serde_json::from_slice::<serde_json::Value>(&tail[..=e]).is_ok())
                        .unwrap_or(false);
                let consumed = if tail_complete {
                    slice.len()
                } else {
                    nl_consumed
                };
                if consumed > 0 {
                    let text = String::from_utf8_lossy(&slice[..consumed]).to_string();
                    off.store(from + consumed as u64, Ordering::Release);
                    sink.emit(&cfg.tab_id, text);
                }
            } else if total < from {
                // File rotated/truncated. Reset to beginning so the next poll re-streams from byte 0 rather than spinning past EOF.
                log::info!(
                    "[tab_chat.raw] file shrank ({} → {}); resetting offset",
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
