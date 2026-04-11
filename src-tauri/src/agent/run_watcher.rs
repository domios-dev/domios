use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::{Emitter, Manager};

use crate::service::ServiceContext;

// Idle hard-coded (claude thinks between events; 30s is enough margin while
// --include-partial-messages keeps bytes flowing during normal generation).
// Hard timeout is config-driven via memory.worker_timeout_seconds.
const IDLE_COMPLETE_SECS: u64 = 30;
const SCANNER_BUFFER_CAP: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub enum AgentRunKind {
    MemWorker { worker_id: String },
    Cron { job_id: String, run_id: String },
}

#[derive(Debug, Default)]
struct CompletionSignal {
    is_error: bool,
    subtype: Option<String>,
    result: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ResultEvent {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    result: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamEventWrapper {
    #[serde(default)]
    event: Option<StreamEventInner>,
}

#[derive(Debug, Deserialize)]
struct StreamEventInner {
    #[serde(rename = "type", default)]
    inner_type: Option<String>,
}

struct RunState {
    pty_id: String,
    pane_id: String,
    kind: AgentRunKind,
    scanner: Mutex<ResultEventScanner>,
    started_at: Instant,
    last_byte_at: Mutex<Instant>,
    completed: AtomicBool,
}

pub struct RunWatcherRegistry {
    runs: Mutex<HashMap<String, Arc<RunState>>>,
}

impl Default for RunWatcherRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RunWatcherRegistry {
    pub fn new() -> Self {
        Self { runs: Mutex::new(HashMap::new()) }
    }

    pub fn register(
        &self,
        svc: ServiceContext,
        pty_id: String,
        pane_id: String,
        kind: AgentRunKind,
        timeout_secs: u64,
    ) {
        let state = Arc::new(RunState {
            pty_id: pty_id.clone(),
            pane_id,
            kind,
            scanner: Mutex::new(ResultEventScanner::new()),
            started_at: Instant::now(),
            last_byte_at: Mutex::new(Instant::now()),
            completed: AtomicBool::new(false),
        });
        self.runs.lock().insert(pty_id, state.clone());
        // Use tauri's runtime handle so this works even when register() is called
        // from outside a tokio reactor (e.g. main thread spawn paths).
        tauri::async_runtime::spawn(watchdog_loop(svc, state, timeout_secs));
    }

    pub fn feed(&self, svc: &ServiceContext, pty_id: &str, bytes: &[u8]) {
        let state = match self.runs.lock().get(pty_id).cloned() {
            Some(s) => s,
            None => return,
        };
        if state.completed.load(Ordering::Acquire) {
            return;
        }
        *state.last_byte_at.lock() = Instant::now();
        let chunk = String::from_utf8_lossy(bytes);
        let signal = state.scanner.lock().feed(&chunk);
        if let Some(signal) = signal {
            if !state.completed.swap(true, Ordering::AcqRel) {
                let summary = signal
                    .result
                    .clone()
                    .unwrap_or_else(|| "(claude completed)".to_string());
                let success = !signal.is_error;
                log::info!(
                    "[run-watcher] completion signal: pane={} success={} subtype={:?} summary_len={}",
                    state.pane_id, success, signal.subtype, summary.len()
                );
                finish_run(svc, &state, success, summary);
                self.runs.lock().remove(&state.pty_id);
            }
        }
    }
}

async fn watchdog_loop(svc: ServiceContext, state: Arc<RunState>, timeout_secs: u64) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        if state.completed.load(Ordering::Acquire) {
            return;
        }
        let elapsed_total = state.started_at.elapsed().as_secs();
        let idle_secs = state.last_byte_at.lock().elapsed().as_secs();

        if idle_secs >= IDLE_COMPLETE_SECS {
            if state.completed.swap(true, Ordering::AcqRel) {
                return;
            }
            log::info!(
                "[run-watcher] idle complete: pane={} idle={}s",
                state.pane_id, idle_secs
            );
            finish_run(
                &svc,
                &state,
                false,
                format!("idle for {}s — claude exited without result event", idle_secs),
            );
            svc.run_watcher.runs.lock().remove(&state.pty_id);
            return;
        }

        if elapsed_total >= timeout_secs {
            if state.completed.swap(true, Ordering::AcqRel) {
                return;
            }
            log::warn!(
                "[run-watcher] hard timeout: pane={} elapsed={}s",
                state.pane_id, elapsed_total
            );
            finish_run(
                &svc,
                &state,
                false,
                format!("hard timeout after {}s", elapsed_total),
            );
            svc.run_watcher.runs.lock().remove(&state.pty_id);
            return;
        }
    }
}

fn finish_run(svc: &ServiceContext, state: &RunState, success: bool, summary: String) {
    let raw = svc
        .output_buf
        .lock()
        .get_full_text(&state.pty_id)
        .unwrap_or_default();
    let clean = crate::pty::ansi::clean_terminal_output(&raw);

    match &state.kind {
        AgentRunKind::MemWorker { worker_id } => {
            if let Ok(path) = crate::mem::workers::save_worker_log(worker_id, &clean) {
                svc.workers
                    .set_log_path(worker_id, path.to_string_lossy().into_owned());
            }
            let _ = svc.workers.complete(worker_id.clone().as_str(), summary);
            let _ = svc.close_pane(&state.pane_id);
        }
        AgentRunKind::Cron { job_id, run_id } => {
            if let Some(cron_state) = svc.app_handle.try_state::<crate::CronState>() {
                let log_text = format!(
                    "--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}",
                    summary, clean
                );
                {
                    let mut store = cron_state.lock();
                    let _ = store.save_log(job_id, run_id, &log_text);
                    let status = if success {
                        crate::cron::store::CronRunStatus::Success
                    } else {
                        crate::cron::store::CronRunStatus::Failed
                    };
                    let _ = store.update_run_status(job_id, run_id, status);
                    store.unregister_pane(&state.pane_id);
                }
                let _ = svc.close_pane(&state.pane_id);
                let _ = svc.app_handle.emit("cron-state-changed", ());
            }
        }
    }
}

// Detects either a top-level result event (no --include-partial-messages mode)
// or a stream_event wrapper containing message_stop (partial-messages mode).
struct ResultEventScanner {
    buffer: String,
}

const RESULT_NEEDLE: &str = "{\"type\":\"result\"";
const STREAM_EVENT_NEEDLE: &str = "{\"type\":\"stream_event\"";

impl ResultEventScanner {
    fn new() -> Self {
        Self { buffer: String::new() }
    }

    fn feed(&mut self, chunk: &str) -> Option<CompletionSignal> {
        let cleaned = crate::pty::ansi::strip_ansi_codes(chunk);
        self.buffer.push_str(&cleaned);

        let mut search_from = 0;
        let mut found: Option<CompletionSignal> = None;
        let mut consumed = 0;

        loop {
            let result_pos = self.buffer[search_from..].find(RESULT_NEEDLE);
            let stream_pos = self.buffer[search_from..].find(STREAM_EVENT_NEEDLE);

            let next = match (result_pos, stream_pos) {
                (None, None) => None,
                (Some(r), None) => Some((search_from + r, true)),
                (None, Some(s)) => Some((search_from + s, false)),
                (Some(r), Some(s)) => {
                    if r <= s {
                        Some((search_from + r, true))
                    } else {
                        Some((search_from + s, false))
                    }
                }
            };

            let Some((start, is_result)) = next else {
                consumed = self.buffer.len();
                break;
            };

            match find_balanced_end(&self.buffer, start) {
                Some(end) => {
                    let candidate = &self.buffer[start..=end];
                    if is_result {
                        if let Ok(parsed) = serde_json::from_str::<ResultEvent>(candidate) {
                            found = Some(CompletionSignal {
                                is_error: parsed.is_error,
                                subtype: parsed.subtype,
                                result: parsed.result,
                            });
                            consumed = end + 1;
                            break;
                        } else {
                            search_from = start + 1;
                        }
                    } else if let Ok(wrapper) = serde_json::from_str::<StreamEventWrapper>(candidate) {
                        if let Some(inner) = wrapper.event {
                            if inner.inner_type.as_deref() == Some("message_stop") {
                                found = Some(CompletionSignal {
                                    is_error: false,
                                    subtype: Some("message_stop".into()),
                                    result: None,
                                });
                                consumed = end + 1;
                                break;
                            }
                        }
                        search_from = end + 1;
                    } else {
                        search_from = start + 1;
                    }
                }
                None => {
                    consumed = start;
                    break;
                }
            }
        }

        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        if self.buffer.len() > SCANNER_BUFFER_CAP {
            let cut = self.buffer.len() - SCANNER_BUFFER_CAP / 2;
            self.buffer = self.buffer.split_off(cut);
        }
        found
    }
}

fn find_balanced_end(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut i = start;
    while i < bytes.len() {
        let ch = bytes[i];
        if esc {
            esc = false;
            i += 1;
            continue;
        }
        if in_str {
            if ch == b'\\' {
                esc = true;
            } else if ch == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == b'"' {
            in_str = true;
        } else if ch == b'{' {
            depth += 1;
        } else if ch == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_finds_result_event_inline() {
        let mut s = ResultEventScanner::new();
        let chunk = r#"some noise {"type":"system","x":1} more {"type":"result","subtype":"success","is_error":false,"result":"hello"}"#;
        let r = s.feed(chunk).expect("completion");
        assert_eq!(r.subtype.as_deref(), Some("success"));
        assert_eq!(r.is_error, false);
        assert_eq!(r.result.as_deref(), Some("hello"));
    }

    #[test]
    fn scanner_finds_message_stop_in_stream_event() {
        let mut s = ResultEventScanner::new();
        let chunk = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"x"}},"session_id":"a"}
{"type":"stream_event","event":{"type":"message_stop"},"session_id":"a","uuid":"b"}"#;
        let r = s.feed(chunk).expect("completion via message_stop");
        assert_eq!(r.subtype.as_deref(), Some("message_stop"));
        assert_eq!(r.is_error, false);
    }

    #[test]
    fn scanner_handles_split_across_chunks() {
        let mut s = ResultEventScanner::new();
        assert!(s.feed(r#"{"type":"result","is_error":false,"#).is_none());
        let r = s.feed(r#""result":"привет"}"#).expect("completion");
        assert_eq!(r.result.as_deref(), Some("привет"));
    }

    #[test]
    fn scanner_strips_ansi() {
        let mut s = ResultEventScanner::new();
        let chunk = "\x1b[32m\x1b]0;title\x07{\"type\":\"result\",\"is_error\":true,\"result\":\"err\"}";
        let r = s.feed(chunk).expect("completion");
        assert!(r.is_error);
        assert_eq!(r.result.as_deref(), Some("err"));
    }

    #[test]
    fn scanner_ignores_non_completion_events() {
        let mut s = ResultEventScanner::new();
        assert!(s
            .feed(r#"{"type":"assistant","message":{"id":"x"}}"#)
            .is_none());
        assert!(s
            .feed(r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#)
            .is_none());
    }

    #[test]
    fn scanner_handles_braces_inside_strings() {
        let mut s = ResultEventScanner::new();
        let chunk =
            r#"{"type":"result","is_error":false,"result":"text with {nested} {{braces}} ok"}"#;
        let r = s.feed(chunk).expect("completion");
        assert_eq!(r.result.as_deref(), Some("text with {nested} {{braces}} ok"));
    }

    #[test]
    fn scanner_caps_buffer_growth() {
        let mut s = ResultEventScanner::new();
        let big = "x".repeat(SCANNER_BUFFER_CAP * 2);
        let _ = s.feed(&big);
        assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
    }
}
