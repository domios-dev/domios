use serde::Deserialize;
use serde_json::{value::RawValue, Value};
use std::borrow::Cow;
use std::collections::HashMap;

const SUMMARY_FRAME_INTERVAL: u64 = 64;
const SUMMARY_BYTE_INTERVAL: u64 = 1024 * 1024;
const MAX_PENDING_FRAME_BYTES: usize = 1024 * 1024;
const PARSER_INPUT_CHUNK_BYTES: usize = 64 * 1024;

pub(crate) fn configured_level(enabled: bool) -> Option<log::Level> {
    choose_level(
        enabled,
        log::log_enabled!(log::Level::Debug),
        log::log_enabled!(log::Level::Info),
    )
}

fn choose_level(enabled: bool, debug_enabled: bool, info_enabled: bool) -> Option<log::Level> {
    if !enabled {
        None
    } else if info_enabled {
        Some(log::Level::Info)
    } else if debug_enabled {
        Some(log::Level::Debug)
    } else {
        None
    }
}

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
    level: log::Level,
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
        level: log::Level,
        side: &'static str,
        session_id: String,
        tab_scope: String,
        cursor_boot: Option<String>,
        cursor_offset: Option<u64>,
    ) -> Self {
        let diagnostics = Self {
            level,
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
        log::log!(
            self.level,
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
        self.observe_frame(FrameSummary {
            tab_id: frame
                .get("tab_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            output_bytes: frame.get("output").and_then(Value::as_str).map(str::len),
            offset: frame.get("offset").and_then(Value::as_u64),
            truncated: frame.get("truncated").and_then(Value::as_bool) == Some(true),
            from: frame.get("from").and_then(Value::as_u64),
            boot_changed: frame.get("boot_changed").and_then(Value::as_bool) == Some(true),
            ended: frame.get("ended").and_then(Value::as_bool) == Some(true),
        });
    }

    pub(crate) fn observe_sse_bytes(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(PARSER_INPUT_CHUNK_BYTES) {
            for frame in self.parser.push(chunk) {
                self.observe_frame(frame);
            }
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

    fn observe_frame(&mut self, frame: FrameSummary) {
        if frame.boot_changed {
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

        let Some(tab_id) = frame.tab_id else {
            return;
        };
        let counters = self.tabs.entry(tab_id.clone()).or_default();
        let mut event = None;
        if let Some(output_bytes) = frame.output_bytes {
            self.counters.observe_output(output_bytes, frame.offset);
            counters.counters.observe_output(output_bytes, frame.offset);
        }
        if frame.truncated {
            self.counters.observe_truncated(frame.from);
            counters.counters.observe_truncated(frame.from);
            event = Some("truncated");
        }
        if frame.ended {
            event = Some("ended");
        }
        if let Some(event) = event {
            self.log_summary(event, "session");
            self.log_tab_summary(&tab_id, event);
        }
        self.log_periodic_if_due();
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
        log::log!(
            self.level,
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
        log::log!(
            self.level,
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
    fn observe_output(&mut self, output_bytes: usize, offset: Option<u64>) {
        self.frames += 1;
        self.bytes += output_bytes as u64;
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

struct FrameSummary {
    tab_id: Option<String>,
    output_bytes: Option<usize>,
    offset: Option<u64>,
    truncated: bool,
    from: Option<u64>,
    boot_changed: bool,
    ended: bool,
}

#[derive(Deserialize)]
struct OutputFrame<'a> {
    #[serde(borrow)]
    tab_id: Option<&'a RawValue>,
    #[serde(borrow)]
    output: Option<&'a RawValue>,
    offset: Option<u64>,
    truncated: Option<bool>,
    from: Option<u64>,
    boot_changed: Option<bool>,
    ended: Option<bool>,
}

#[derive(Default)]
struct SseFrameParser {
    pending: Vec<u8>,
    scan_from: usize,
    discarding: bool,
}

impl SseFrameParser {
    fn push(&mut self, chunk: &[u8]) -> Vec<FrameSummary> {
        let mut frames = Vec::new();
        for part in chunk.chunks(PARSER_INPUT_CHUNK_BYTES) {
            self.push_part(part, &mut frames);
        }
        frames
    }

    fn push_part(&mut self, chunk: &[u8], frames: &mut Vec<FrameSummary>) {
        self.pending.extend_from_slice(chunk);
        if self.discarding {
            let Some((end, separator_len)) = find_separator(&self.pending, self.scan_from) else {
                self.retain_separator_suffix();
                return;
            };
            self.pending.drain(..end + separator_len);
            self.discarding = false;
            self.scan_from = 0;
        }

        let mut consumed = 0;
        let mut scan_from = self.scan_from;
        while let Some((end, separator_len)) = find_separator(&self.pending, scan_from) {
            if let Some(frame) = parse_data_frame(&self.pending[consumed..end]) {
                frames.push(frame);
            }
            consumed = end + separator_len;
            scan_from = consumed;
        }
        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        self.scan_from = self.pending.len().saturating_sub(3);
        if self.pending.len() > MAX_PENDING_FRAME_BYTES {
            self.retain_separator_suffix();
            self.discarding = true;
        }
    }

    fn retain_separator_suffix(&mut self) {
        let keep_from = self.pending.len().saturating_sub(3);
        if keep_from > 0 {
            self.pending.drain(..keep_from);
        }
        self.scan_from = 0;
    }
}

fn find_separator(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut index = start.min(bytes.len());
    while index < bytes.len() {
        if bytes.get(index..index + 2) == Some(&b"\n\n"[..]) {
            return Some((index, 2));
        }
        if bytes.get(index..index + 4) == Some(&b"\r\n\r\n"[..]) {
            return Some((index, 4));
        }
        index += 1;
    }
    None
}

fn parse_data_frame(frame: &[u8]) -> Option<FrameSummary> {
    let data = data_payload(frame)?;
    let parsed: OutputFrame<'_> = serde_json::from_slice(&data).ok()?;
    Some(FrameSummary {
        tab_id: parsed
            .tab_id
            .and_then(|tab| serde_json::from_str(tab.get()).ok()),
        output_bytes: parsed
            .output
            .and_then(|output| json_string_bytes(output.get())),
        offset: parsed.offset,
        truncated: parsed.truncated == Some(true),
        from: parsed.from,
        boot_changed: parsed.boot_changed == Some(true),
        ended: parsed.ended == Some(true),
    })
}

fn data_payload(frame: &[u8]) -> Option<Cow<'_, [u8]>> {
    let mut data_lines = frame.split(|byte| *byte == b'\n').filter_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.strip_prefix(b"data:")
            .map(|payload| payload.strip_prefix(b" ").unwrap_or(payload))
    });
    let first = data_lines.next()?;
    let Some(second) = data_lines.next() else {
        return Some(Cow::Borrowed(first));
    };

    let mut data = Vec::with_capacity(first.len() + second.len() + 1);
    data.extend_from_slice(first);
    if !data.is_empty() {
        data.push(b'\n');
    }
    data.extend_from_slice(second);
    for line in data_lines {
        if !data.is_empty() {
            data.push(b'\n');
        }
        data.extend_from_slice(line);
    }
    Some(Cow::Owned(data))
}

fn json_string_bytes(raw: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return None;
    }
    if !bytes.contains(&b'\\') {
        return Some(bytes.len() - 2);
    }

    let end = bytes.len() - 1;
    let mut index = 1;
    let mut decoded_len = 0;
    while index < end {
        if bytes[index] != b'\\' {
            decoded_len += 1;
            index += 1;
            continue;
        }
        index += 1;
        match *bytes.get(index)? {
            b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                decoded_len += 1;
                index += 1;
            }
            b'u' => {
                let code = parse_hex_quad(bytes.get(index + 1..index + 5)?)?;
                index += 5;
                if (0xD800..=0xDBFF).contains(&code)
                    && bytes.get(index..index + 2) == Some(&b"\\u"[..])
                {
                    let low = parse_hex_quad(bytes.get(index + 2..index + 6)?)?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    decoded_len += 4;
                    index += 6;
                } else if (0xD800..=0xDFFF).contains(&code) {
                    return None;
                } else {
                    decoded_len += match code {
                        0x0000..=0x007F => 1,
                        0x0080..=0x07FF => 2,
                        0x0800..=0xFFFF => 3,
                    };
                }
            }
            _ => return None,
        }
    }
    Some(decoded_len)
}

fn parse_hex_quad(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    bytes.iter().try_fold(0u16, |value, byte| {
        let digit = match *byte {
            b'0'..=b'9' => (*byte - b'0') as u16,
            b'a'..=b'f' => (*byte - b'a' + 10) as u16,
            b'A'..=b'F' => (*byte - b'A' + 10) as u16,
            _ => return None,
        };
        Some((value << 4) | digit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_observer_counts_offsets_and_gap_frames_across_chunks() {
        let mut diagnostics = StreamDiagnostics::new(
            log::Level::Debug,
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
            log::Level::Debug,
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

    #[test]
    fn disabled_diagnostics_skip_observation_and_enabled_runtime_uses_info() {
        assert_eq!(choose_level(false, false, false), None);
        assert_eq!(choose_level(false, true, true), None);
        assert_eq!(choose_level(false, true, false), None);
        assert_eq!(choose_level(true, true, true), Some(log::Level::Info));
        assert_eq!(choose_level(true, true, false), Some(log::Level::Debug));
        assert_eq!(choose_level(true, false, false), None);
    }

    #[test]
    fn runtime_diagnostics_config_defaults_off_and_accepts_live_config_value() {
        let default = crate::config::schema::AppConfig::default();
        assert!(!default.diagnostics.terminal_transport);

        let enabled: crate::config::schema::AppConfig =
            toml::from_str("[diagnostics]\nterminal_transport = true\n").unwrap();
        assert!(enabled.diagnostics.terminal_transport);
        assert!(toml::to_string(&enabled)
            .unwrap()
            .contains("terminal_transport = true"));
    }

    #[test]
    fn parser_discards_oversize_frame_and_recovers_after_separator() {
        let mut parser = SseFrameParser::default();
        let oversized = vec![b'x'; MAX_PENDING_FRAME_BYTES + 1];
        assert!(parser.push(&oversized).is_empty());
        assert!(parser.discarding);
        assert!(parser.pending.len() <= 3);

        let recovered =
            parser.push(b"\n\ndata: {\"tab_id\":\"tab-1\",\"output\":\"ok\",\"offset\":2}\n\n");
        assert!(!parser.discarding);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].tab_id.as_deref(), Some("tab-1"));
        assert_eq!(recovered[0].output_bytes, Some(2));
        assert_eq!(recovered[0].offset, Some(2));
    }

    #[test]
    fn output_length_counts_json_escapes_without_decoding_the_text() {
        assert_eq!(json_string_bytes(r#""a\n\uD83D\uDE00""#), Some(6));
        assert_eq!(json_string_bytes(r#""plain 😀""#), Some(10));
        assert_eq!(json_string_bytes(r#""a\"b""#), Some(3));
        assert_eq!(json_string_bytes(r#""a\\b""#), Some(3));
        assert_eq!(json_string_bytes(r#""\uD800""#), None);
        assert_eq!(json_string_bytes("true"), None);
    }
}
