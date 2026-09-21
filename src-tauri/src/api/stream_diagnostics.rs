use serde_json::Value;
use std::collections::HashMap;

const SUMMARY_FRAME_INTERVAL: u64 = 64;
const SUMMARY_BYTE_INTERVAL: u64 = 1024 * 1024;
const MAX_PENDING_FRAME_BYTES: usize = 1024 * 1024;

pub(crate) struct StreamTabStart {
    pub tab_id: String,
    pub pty_id: Option<String>,
    pub cursor_boot: Option<String>,
    pub cursor_offset: Option<u64>,
    pub total_written: Option<u64>,
    pub resume_offset: Option<u64>,
    pub replay_truncated: Option<bool>,
    pub replay_truncated_from: Option<u64>,
    pub boot_changed: bool,
}

#[derive(Default)]
struct TabCounters {
    pty_id: Option<String>,
    cursor_boot: Option<String>,
    cursor_offset: Option<u64>,
    total_written: Option<u64>,
    resume_offset: Option<u64>,
    replay_truncated: Option<bool>,
    replay_truncated_from: Option<u64>,
    boot_changed_at_open: bool,
    counters: StreamCounters,
}

#[derive(Default)]
struct StreamCounters {
    frames: u64,
    bytes: u64,
    first_offset: Option<u64>,
    last_offset: Option<u64>,
    truncated_events: u64,
    truncated_from: Option<u64>,
    boot_changed_events: u64,
    last_reported_frames: u64,
    last_reported_bytes: u64,
}

pub(crate) struct StreamDiagnostics {
    side: &'static str,
    session_id: String,
    tab_scope: String,
    cursor_boot: Option<String>,
    cursor_offset: Option<u64>,
    counters: StreamCounters,
    tabs: HashMap<String, TabCounters>,
    parser: SseFrameParser,
    last_reported_frames: u64,
    last_reported_bytes: u64,
    close_reason: &'static str,
    finished: bool,
}

impl StreamDiagnostics {
    pub(crate) fn new(
        side: &'static str,
        session_id: String,
        tab_scope: String,
        cursor_boot: Option<String>,
        cursor_offset: Option<u64>,
    ) -> Self {
        let diagnostics = Self {
            side,
            session_id,
            tab_scope,
            cursor_boot,
            cursor_offset,
            counters: StreamCounters::default(),
            tabs: HashMap::new(),
            parser: SseFrameParser::default(),
            last_reported_frames: 0,
            last_reported_bytes: 0,
            close_reason: "cancelled",
            finished: false,
        };
        diagnostics.log_summary("open", "session");
        diagnostics
    }

    pub(crate) fn attach_tab(&mut self, start: StreamTabStart) {
        let counters = TabCounters {
            pty_id: start.pty_id,
            cursor_boot: start.cursor_boot,
            cursor_offset: start.cursor_offset,
            total_written: start.total_written,
            resume_offset: start.resume_offset,
            replay_truncated: start.replay_truncated,
            replay_truncated_from: start.replay_truncated_from,
            boot_changed_at_open: start.boot_changed,
            ..TabCounters::default()
        };
        log::debug!(
            "[terminal-stream] event=attach side={} session={} tab={} pty_id={:?} cursor_boot={:?} cursor_offset={:?} total_written={:?} resume_offset={:?} replay_truncated={:?} replay_truncated_from={:?} boot_changed={}",
            self.side,
            self.session_id,
            start.tab_id,
            counters.pty_id,
            counters.cursor_boot,
            counters.cursor_offset,
            counters.total_written,
            counters.resume_offset,
            counters.replay_truncated,
            counters.replay_truncated_from,
            counters.boot_changed_at_open,
        );
        self.tabs.insert(start.tab_id, counters);
    }

    pub(crate) fn observe_value(&mut self, frame: &Value) {
        if frame.get("boot_changed").and_then(Value::as_bool) == Some(true) {
            self.counters.boot_changed_events += 1;
            for counters in self.tabs.values_mut() {
                counters.counters.boot_changed_events += 1;
            }
            self.log_summary("boot_changed", "session");
            for tab in self.tabs.keys() {
                self.log_tab_summary(tab, "boot_changed");
            }
            return;
        }

        let Some(tab_id) = frame.get("tab_id").and_then(Value::as_str) else {
            return;
        };
        let counters = self.tabs.entry(tab_id.to_string()).or_default();
        let mut event = None;
        if let Some(output) = frame.get("output").and_then(Value::as_str) {
            let offset = frame.get("offset").and_then(Value::as_u64);
            self.counters.observe_output(output, offset);
            counters.counters.observe_output(output, offset);
        }
        if frame.get("truncated").and_then(Value::as_bool) == Some(true) {
            let from = frame.get("from").and_then(Value::as_u64);
            self.counters.observe_truncated(from);
            counters.counters.observe_truncated(from);
            event = Some("truncated");
        }
        if frame.get("ended").and_then(Value::as_bool) == Some(true) {
            event = Some("ended");
        }
        if let Some(event) = event {
            self.log_summary(event, "session");
            self.log_tab_summary(tab_id, event);
        }
        self.log_periodic_if_due();
    }

    pub(crate) fn observe_sse_bytes(&mut self, bytes: &[u8]) {
        for frame in self.parser.push(bytes) {
            self.observe_value(&frame);
        }
    }

    pub(crate) fn finish(&mut self, reason: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.close_reason = reason;
        self.log_summary("close", reason);
        for tab in self.tabs.keys() {
            self.log_tab_summary(tab, reason);
        }
    }

    fn log_periodic_if_due(&mut self) {
        let frames_due = self.counters.frames - self.last_reported_frames >= SUMMARY_FRAME_INTERVAL;
        let bytes_due = self.counters.bytes - self.last_reported_bytes >= SUMMARY_BYTE_INTERVAL;
        if !frames_due && !bytes_due {
            return;
        }
        self.last_reported_frames = self.counters.frames;
        self.last_reported_bytes = self.counters.bytes;
        self.log_summary("periodic", "threshold");
        let tabs: Vec<String> = self.tabs.keys().cloned().collect();
        for tab in tabs {
            let due = if let Some(counters) = self.tabs.get_mut(&tab) {
                let counts = &mut counters.counters;
                let due = counts.frames - counts.last_reported_frames >= SUMMARY_FRAME_INTERVAL
                    || counts.bytes - counts.last_reported_bytes >= SUMMARY_BYTE_INTERVAL;
                if due {
                    counts.last_reported_frames = counts.frames;
                    counts.last_reported_bytes = counts.bytes;
                }
                due
            } else {
                false
            };
            if due {
                self.log_tab_summary(&tab, "periodic");
            }
        }
    }

    fn log_summary(&self, event: &str, reason: &str) {
        log::debug!(
            "[terminal-stream] event={} side={} session={} tab={} reason={} cursor_boot={:?} cursor_offset={:?} frames={} bytes={} first_offset={:?} last_offset={:?} truncated_events={} truncated_from={:?} boot_changed_events={}",
            event,
            self.side,
            self.session_id,
            self.tab_scope,
            reason,
            self.cursor_boot,
            self.cursor_offset,
            self.counters.frames,
            self.counters.bytes,
            self.counters.first_offset,
            self.counters.last_offset,
            self.counters.truncated_events,
            self.counters.truncated_from,
            self.counters.boot_changed_events,
        );
    }

    fn log_tab_summary(&self, tab: &str, event: &str) {
        let Some(counters) = self.tabs.get(tab) else {
            return;
        };
        log::debug!(
            "[terminal-stream] event={} side={} session={} tab={} pty_id={:?} cursor_boot={:?} cursor_offset={:?} total_written={:?} resume_offset={:?} frames={} bytes={} first_offset={:?} last_offset={:?} truncated_events={} truncated_from={:?} replay_truncated={:?} replay_truncated_from={:?} boot_changed_at_open={} boot_changed_events={}",
            event,
            self.side,
            self.session_id,
            tab,
            counters.pty_id,
            counters.cursor_boot,
            counters.cursor_offset,
            counters.total_written,
            counters.resume_offset,
            counters.counters.frames,
            counters.counters.bytes,
            counters.counters.first_offset,
            counters.counters.last_offset,
            counters.counters.truncated_events,
            counters.counters.truncated_from,
            counters.replay_truncated,
            counters.replay_truncated_from,
            counters.boot_changed_at_open,
            counters.counters.boot_changed_events,
        );
    }
}

impl Drop for StreamDiagnostics {
    fn drop(&mut self) {
        self.finish(self.close_reason);
    }
}

impl StreamCounters {
    fn observe_output(&mut self, output: &str, offset: Option<u64>) {
        self.frames += 1;
        self.bytes += output.len() as u64;
        if let Some(offset) = offset {
            self.first_offset.get_or_insert(offset);
            self.last_offset = Some(offset);
        }
    }

    fn observe_truncated(&mut self, from: Option<u64>) {
        self.truncated_events += 1;
        self.truncated_from = from;
    }
}

#[derive(Default)]
struct SseFrameParser {
    pending: Vec<u8>,
    discarding: bool,
}

impl SseFrameParser {
    fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        if self.discarding {
            let mut buffered = std::mem::take(&mut self.pending);
            buffered.extend_from_slice(chunk);
            let Some((end, separator_len)) = find_separator(&buffered) else {
                let start = buffered.len().saturating_sub(3);
                self.pending.extend_from_slice(&buffered[start..]);
                return Vec::new();
            };
            self.discarding = false;
            self.pending
                .extend_from_slice(&buffered[end + separator_len..]);
        } else {
            self.pending.extend_from_slice(chunk);
        }

        let mut frames = Vec::new();
        while let Some((end, separator_len)) = find_separator(&self.pending) {
            let frame = self.pending[..end].to_vec();
            self.pending.drain(..end + separator_len);
            if let Some(value) = parse_data_frame(&frame) {
                frames.push(value);
            }
        }
        if self.pending.len() > MAX_PENDING_FRAME_BYTES {
            let start = self.pending.len().saturating_sub(3);
            let tail = self.pending[start..].to_vec();
            self.pending.clear();
            self.pending.extend_from_slice(&tail);
            self.discarding = true;
        }
        frames
    }
}

fn find_separator(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes.windows(2).position(|part| part == b"\n\n");
    let crlf = bytes.windows(4).position(|part| part == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        (None, None) => None,
    }
}

fn parse_data_frame(frame: &[u8]) -> Option<Value> {
    let mut data = Vec::new();
    for line in frame.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(payload) = line.strip_prefix(b"data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push(b'\n');
        }
        data.extend_from_slice(payload.strip_prefix(b" ").unwrap_or(payload));
    }
    serde_json::from_slice(&data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_observer_counts_offsets_and_gap_frames_across_chunks() {
        let mut diagnostics = StreamDiagnostics::new(
            "relay",
            "session-1".into(),
            "tab-1".into(),
            Some("boot-1".into()),
            Some(0),
        );
        diagnostics.attach_tab(StreamTabStart {
            tab_id: "tab-1".into(),
            pty_id: None,
            cursor_boot: Some("boot-1".into()),
            cursor_offset: Some(0),
            total_written: None,
            resume_offset: Some(0),
            replay_truncated: None,
            replay_truncated_from: None,
            boot_changed: false,
        });

        diagnostics
            .observe_sse_bytes(b"data: {\"tab_id\":\"tab-1\",\"output\":\"hello\",\"offset\":5}\n");
        assert_eq!(diagnostics.counters.frames, 0);
        diagnostics.observe_sse_bytes(b"\n");
        diagnostics.observe_sse_bytes(
            b"data: {\"tab_id\":\"tab-1\",\"truncated\":true,\"from\":8}\r\n\r\n",
        );

        assert_eq!(diagnostics.counters.frames, 1);
        assert_eq!(diagnostics.counters.bytes, 5);
        assert_eq!(diagnostics.counters.first_offset, Some(5));
        assert_eq!(diagnostics.counters.last_offset, Some(5));
        assert_eq!(diagnostics.counters.truncated_events, 1);
        assert_eq!(diagnostics.counters.truncated_from, Some(8));
        let tab = diagnostics.tabs.get("tab-1").unwrap();
        assert_eq!(tab.counters.frames, 1);
        assert_eq!(tab.counters.bytes, 5);
        assert_eq!(tab.counters.truncated_events, 1);
    }

    #[test]
    fn sse_observer_counts_boot_change_control_frame() {
        let mut diagnostics = StreamDiagnostics::new(
            "peer",
            "session-2".into(),
            "tab-2".into(),
            Some("old-boot".into()),
            Some(12),
        );
        diagnostics.observe_value(&serde_json::json!({
            "boot_changed": true,
            "boot": "new-boot",
        }));
        assert_eq!(diagnostics.counters.boot_changed_events, 1);
    }
}
