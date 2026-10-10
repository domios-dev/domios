use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::registry::{JsonlSource, SourceIncarnation, SourceObservation};
use crate::agent::chat_types::RichMessage;
use crate::agent::registry::AgentRegistry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRange {
    pub start_offset: u64,
    pub offset: u64,
    pub incarnation: Option<SourceIncarnation>,
}

/// Caller-supplied sink. Real impl emits a Tauri event with the
/// messages; tests pass a `Vec<RichMessage>` collector.
pub trait DeltaSink: Send + Sync {
    fn emit(&self, tab_id: &str, messages: Vec<RichMessage>, start_offset: u64, offset: u64);

    fn emit_sourced(&self, tab_id: &str, messages: Vec<RichMessage>, range: SourceRange) {
        self.emit(tab_id, messages, range.start_offset, range.offset);
    }
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
    wake: Option<SyncSender<()>>,
}

impl TailerHandle {
    /// Current byte offset into the session JSONL. Useful for backfill —
    /// the REST `chat/backfill` endpoint reads the range `[offset - N, offset)`.
    pub fn current_offset(&self) -> u64 {
        self.offset.load(Ordering::Acquire)
    }

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
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
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
            wake: None,
        }
    }

    pub(crate) fn session_owned(&self) -> bool {
        self.wake.is_some()
    }

    pub(crate) fn event_driven(
        stop_flag: Arc<AtomicBool>,
        offset: Arc<AtomicU64>,
        join: thread::JoinHandle<()>,
        wake: SyncSender<()>,
    ) -> Self {
        Self {
            stop_flag,
            offset,
            join: Mutex::new(Some(join)),
            wake: Some(wake),
        }
    }
}

impl Drop for TailerHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
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
}

pub fn spawn_tailer(
    cfg: TailConfig,
    registry: Arc<AgentRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> TailerHandle {
    spawn_sourced_tailer(cfg, None, registry, sink)
}

pub(crate) fn spawn_sourced_tailer(
    cfg: TailConfig,
    source: Option<Arc<JsonlSource>>,
    registry: Arc<AgentRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let offset = Arc::new(AtomicU64::new(cfg.start_offset));

    let stop = stop_flag.clone();
    let off = offset.clone();
    let (wake, events) = mpsc::sync_channel(1);
    let thread_wake = wake.clone();
    let join = thread::Builder::new()
        .name(format!("chat-jsonl-{}", cfg.tab_id))
        .spawn(move || {
            let path = registry
                .with_provider(&cfg.provider_id, |provider| {
                    provider.session_jsonl_path(&cfg.cwd, &cfg.session_id)
                })
                .flatten();
            let Some(path) = path else {
                log::warn!("[tab_chat] no JSONL path for tab={}", cfg.tab_id);
                return;
            };
            let _watcher = match super::file_events::watch(&path, thread_wake.clone()) {
                Ok(watcher) => watcher,
                Err(error) => {
                    log::error!("[tab_chat] tab={} watcher failed: {}", cfg.tab_id, error);
                    return;
                }
            };
            log::info!("[tab_chat] tab={} tailing {}", cfg.tab_id, path.display());
            let _ = thread_wake.try_send(());
            let mut source_incarnation: Option<SourceIncarnation> = None;
            while events.recv().is_ok() {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let observation = source.as_ref().map(|source| {
                    source.observe(
                        &cfg.session_id,
                        &path,
                        source_incarnation.as_ref(),
                        off.load(Ordering::Acquire),
                    )
                });
                let (exists, restart) = match observation {
                    Some(SourceObservation::Present {
                        incarnation,
                        restart,
                    }) => {
                        source_incarnation = incarnation;
                        (true, restart)
                    }
                    Some(SourceObservation::Absent) => (false, false),
                    None => (path.exists(), false),
                };
                if !exists {
                    continue;
                }
                if restart {
                    off.store(0, Ordering::Release);
                }
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
                    if delta.new_offset > from
                        && std::fs::metadata(&path).is_ok_and(|file| file.len() > delta.new_offset)
                    {
                        let _ = thread_wake.try_send(());
                    }
                }
                if !delta.messages.is_empty() {
                    sink.emit_sourced(
                        &cfg.tab_id,
                        delta.messages,
                        SourceRange {
                            start_offset: from,
                            offset: delta.new_offset,
                            incarnation: source_incarnation.clone(),
                        },
                    );
                }
            }
        })
        .expect("chat worker thread could not start");

    TailerHandle::event_driven(stop_flag, offset, join, wake)
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
