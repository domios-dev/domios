use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;
use tauri::{Emitter, Manager};

use crate::service::ServiceContext;

// Default idle wait is 30s (Claude can think between events while still streaming partials).
// `memory.worker_timeout_seconds` can lower/higher this at runtime.
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
        Self {
            runs: Mutex::new(HashMap::new()),
        }
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
        // Use tauri's runtime handle so this works even when register() is called from outside a tokio reactor (e.g. main thread spawn paths).
        tauri::async_runtime::spawn(watchdog_loop(svc, state, timeout_secs));
    }

    /// Tell the watcher this run is already finished via an explicit
    /// completion path (e.g. `codeterm cron complete`) so the idle / hard
    /// timeout watchdog does NOT fire afterwards and clobber the Success
    /// status with Failed.
    pub fn mark_completed_externally(&self, pty_id: &str) {
        let mut runs = self.runs.lock();
        if let Some(state) = runs.get(pty_id) {
            state.completed.store(true, Ordering::Release);
        }
        runs.remove(pty_id);
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
            // Defer the kill while a tool call is mid-flight: claude's PTY can stay silent for >30s during a slow Bash subprocess (mem graph hubs on a
            // busy graph, large `mem search`, etc.) even though the run is alive. Hard timeout still bounds runaway tools.
            let busy = state.scanner.lock().in_flight();
            if busy {
                log::debug!(
                    "[run-watcher] idle {}s but tool in-flight — deferring reap: pane={}",
                    idle_secs,
                    state.pane_id
                );
            } else {
                if state.completed.swap(true, Ordering::AcqRel) {
                    return;
                }
                log::info!(
                    "[run-watcher] idle complete: pane={} idle={}s",
                    state.pane_id,
                    idle_secs
                );
                finish_run(
                    &svc,
                    &state,
                    false,
                    format!(
                        "idle for {}s — claude exited without result event",
                        idle_secs
                    ),
                );
                svc.run_watcher.runs.lock().remove(&state.pty_id);
                return;
            }
        }

        if elapsed_total >= timeout_secs {
            if state.completed.swap(true, Ordering::AcqRel) {
                return;
            }
            log::warn!(
                "[run-watcher] hard timeout: pane={} elapsed={}s",
                state.pane_id,
                elapsed_total
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

// Track only top-level run completion events (`{"type":"result"}`), not nested `message_stop`.
// Tool lifecycle is in-flight tracked by `tool_use`/`tool_result`, so we skip idle reaping while tools run.
struct ResultEventScanner {
    buffer: String,
    tool_scan_pos: usize,
    in_flight_tools: HashSet<String>,
}

const RESULT_NEEDLE: &str = "{\"type\":\"result\"";
const TOOL_USE_NEEDLE: &str = "{\"type\":\"tool_use\"";
const TOOL_RESULT_NEEDLE: &str = "{\"type\":\"tool_result\"";

#[derive(Debug, Deserialize, Default)]
struct ToolUseHeader {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ToolResultHeader {
    #[serde(default)]
    tool_use_id: Option<String>,
}

impl ResultEventScanner {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            tool_scan_pos: 0,
            in_flight_tools: HashSet::new(),
        }
    }

    fn in_flight(&self) -> bool {
        !self.in_flight_tools.is_empty()
    }

    fn feed(&mut self, chunk: &str) -> Option<CompletionSignal> {
        let cleaned = crate::pty::ansi::strip_ansi_codes(chunk);
        self.buffer.push_str(&cleaned);

        // Pass 1: tool-use / tool-result lifecycle. Done first so a buffer drain after the result-event pass below cannot lose unscanned tool events.
        self.scan_tool_events();

        let mut search_from = 0;
        let mut found: Option<CompletionSignal> = None;
        // Floor the drain at the tool scanner's cursor: it may be parked mid-object waiting for the closing brace to arrive in the next chunk, and
        // dropping bytes past that cursor would lose the partial tool_use we're tracking.
        let mut consumed = 0usize;

        loop {
            let Some(start_rel) = self.buffer[search_from..].find(RESULT_NEEDLE) else {
                break;
            };
            let start = search_from + start_rel;
            match find_balanced_end(&self.buffer, start) {
                Some(end) => {
                    let candidate = &self.buffer[start..=end];
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
                }
                None => {
                    break;
                }
            }
        }

        let safe_consume = consumed.min(self.tool_scan_pos);
        if safe_consume > 0 {
            self.buffer.drain(..safe_consume);
            self.tool_scan_pos = self.tool_scan_pos.saturating_sub(safe_consume);
        }
        if self.buffer.len() > SCANNER_BUFFER_CAP {
            let cut = self.buffer.len() - SCANNER_BUFFER_CAP / 2;
            self.buffer = self.buffer.split_off(cut);
            self.tool_scan_pos = self.tool_scan_pos.saturating_sub(cut);
        }
        found
    }

    fn scan_tool_events(&mut self) {
        let mut pos = self.tool_scan_pos;
        let mut max_consumed = pos;
        loop {
            let next_use = self.buffer[pos..].find(TOOL_USE_NEEDLE).map(|r| pos + r);
            let next_result = self.buffer[pos..].find(TOOL_RESULT_NEEDLE).map(|r| pos + r);

            // Pick whichever needle comes first.
            let (start, is_use) = match (next_use, next_result) {
                (Some(u), Some(r)) if u < r => (u, true),
                (Some(_), Some(r)) => (r, false),
                (Some(u), None) => (u, true),
                (None, Some(r)) => (r, false),
                (None, None) => break,
            };

            match find_balanced_end(&self.buffer, start) {
                Some(end) => {
                    let candidate = &self.buffer[start..=end];
                    if is_use {
                        if let Ok(parsed) = serde_json::from_str::<ToolUseHeader>(candidate) {
                            if let Some(id) = parsed.id {
                                self.in_flight_tools.insert(id);
                            }
                        }
                    } else {
                        if let Ok(parsed) = serde_json::from_str::<ToolResultHeader>(candidate) {
                            if let Some(id) = parsed.tool_use_id {
                                self.in_flight_tools.remove(&id);
                            }
                        }
                    }
                    pos = end + 1;
                    max_consumed = pos;
                }
                None => {
                    // Object incomplete — leave tool_scan_pos here so the next chunk re-attempts from the same start.
                    break;
                }
            }
        }
        self.tool_scan_pos = max_consumed;
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
    fn scanner_ignores_mid_stream_message_stop() {
        // message_stop inside stream_event fires after every assistant turn in a tool-use loop — must NOT be treated as completion or the watcher
        // kills the pane before claude can process tool results.
        let mut s = ResultEventScanner::new();
        let chunk = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"x"}}}
{"type":"stream_event","event":{"type":"message_stop"},"session_id":"a","uuid":"b"}"#;
        assert!(
            s.feed(chunk).is_none(),
            "must not complete on nested message_stop"
        );
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
        let chunk =
            "\x1b[32m\x1b]0;title\x07{\"type\":\"result\",\"is_error\":true,\"result\":\"err\"}";
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
        assert_eq!(
            r.result.as_deref(),
            Some("text with {nested} {{braces}} ok")
        );
    }

    #[test]
    fn scanner_caps_buffer_growth() {
        let mut s = ResultEventScanner::new();
        let big = "x".repeat(SCANNER_BUFFER_CAP * 2);
        let _ = s.feed(&big);
        assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
    }

    #[test]
    fn scanner_in_flight_starts_false() {
        let s = ResultEventScanner::new();
        assert!(!s.in_flight());
    }

    #[test]
    fn scanner_marks_tool_use_in_flight() {
        let mut s = ResultEventScanner::new();
        let chunk = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_ABC","name":"Bash","input":{}}]}}"#;
        let _ = s.feed(chunk);
        assert!(s.in_flight(), "tool_use should mark scanner busy");
    }

    #[test]
    fn scanner_clears_on_tool_result() {
        let mut s = ResultEventScanner::new();
        let _ = s.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_X","name":"Bash","input":{}}]}}"#);
        assert!(s.in_flight());
        let _ = s.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_X","content":"done"}]}}"#);
        assert!(
            !s.in_flight(),
            "matching tool_result should clear in-flight"
        );
    }

    #[test]
    fn scanner_tracks_parallel_tools() {
        let mut s = ResultEventScanner::new();
        let _ = s.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_A","name":"Bash","input":{}},{"type":"tool_use","id":"toolu_B","name":"Read","input":{}}]}}"#);
        assert!(s.in_flight());
        let _ = s.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_A","content":"ok"}]}}"#);
        assert!(s.in_flight(), "second tool still pending");
        let _ = s.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_B","content":"ok"}]}}"#);
        assert!(!s.in_flight(), "all tools resolved");
    }

    #[test]
    fn scanner_handles_split_tool_use_across_chunks() {
        let mut s = ResultEventScanner::new();
        let _ =
            s.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_"#);
        assert!(!s.in_flight(), "incomplete object — id not yet visible");
        let _ = s.feed(r#"SPLIT","name":"Bash","input":{}}]}}"#);
        assert!(s.in_flight(), "completes once the closing brace arrives");
    }

    #[test]
    fn scanner_picks_up_partial_message_content_block() {
        // --include-partial-messages emits content_block_start with the tool_use shape nested inside. We must catch that form too.
        let mut s = ResultEventScanner::new();
        let chunk = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_PM","name":"Bash","input":{}}}}"#;
        let _ = s.feed(chunk);
        assert!(
            s.in_flight(),
            "partial-message tool_use must register in-flight"
        );
    }

    #[test]
    fn scanner_result_event_still_works_with_tool_tracking() {
        // Sanity: extending the scanner must not break the existing top-level result detection.
        let mut s = ResultEventScanner::new();
        let _ = s.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_F","name":"Bash","input":{}}]}}"#);
        let _ = s.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_F","content":"ok"}]}}"#);
        let r = s
            .feed(r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#)
            .expect("completion");
        assert_eq!(r.subtype.as_deref(), Some("success"));
        assert!(!s.in_flight());
    }
}
