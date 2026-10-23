use axum::body::Bytes;
use axum::http::Uri;
use serde::Serialize;
use serde_json::value::RawValue;

/// Inner frames must still fit the browser's 2 MiB SSE frame bound once wrapped.
pub(crate) const CHANNEL_FRAME_CAP: usize = 2 * 1024 * 1024 - 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MuxTarget {
    Relay(Uri),
    LocalOutput(Uri),
}

pub(crate) fn classify_channel_path(path: &str) -> Result<MuxTarget, String> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains('\\') {
        return Err("Mux channel path must be origin-relative".into());
    }
    let uri: Uri = path
        .parse()
        .map_err(|_| format!("Invalid mux channel path: {path}"))?;
    match uri.path() {
        "/api/mesh/relay-stream" => Ok(MuxTarget::Relay(uri)),
        "/api/output/stream" => Ok(MuxTarget::LocalOutput(uri)),
        other => Err(format!("Not a multiplexable stream path: {other}")),
    }
}

pub(crate) fn valid_mux_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseFrame {
    pub event: Option<String>,
    /// `None` for comment-only frames such as keep-alives.
    pub data: Option<String>,
}

pub(crate) struct SseSplitter {
    line: Vec<u8>,
    event: Option<String>,
    data: Option<String>,
    has_fields: bool,
    frame_bytes: usize,
    cap: usize,
}

impl SseSplitter {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            line: Vec::new(),
            event: None,
            data: None,
            has_fields: false,
            frame_bytes: 0,
            cap,
        }
    }

    /// Splits upstream bytes into complete frames; `Err` carries the oversized frame length.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, usize> {
        let mut frames = Vec::new();
        for &byte in chunk {
            self.frame_bytes += 1;
            if self.frame_bytes > self.cap {
                return Err(self.frame_bytes);
            }
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
            let line = std::mem::take(&mut self.line);
            if line.is_empty() {
                if self.has_fields {
                    frames.push(SseFrame {
                        event: self.event.take(),
                        data: self.data.take(),
                    });
                }
                self.has_fields = false;
                self.frame_bytes = 0;
                continue;
            }
            self.has_fields = true;
            if line[0] == b':' {
                continue;
            }
            let text = String::from_utf8_lossy(&line);
            let (field, value) = match text.find(':') {
                Some(at) => (
                    &text[..at],
                    text[at + 1..].strip_prefix(' ').unwrap_or(&text[at + 1..]),
                ),
                None => (&text[..], ""),
            };
            match field {
                "data" => match self.data.as_mut() {
                    Some(data) => {
                        data.push('\n');
                        data.push_str(value);
                    }
                    None => self.data = Some(value.to_string()),
                },
                "event" => self.event = Some(value.to_string()),
                _ => {}
            }
        }
        Ok(frames)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Payload<'a> {
    Raw(&'a RawValue),
    Value(serde_json::Value),
    Text(&'a str),
}

fn payload(data: &str) -> Payload<'_> {
    // RawValue keeps original whitespace, so multi-line JSON is re-serialized to stay one SSE line.
    if !data.contains(['\n', '\r']) {
        if let Ok(raw) = serde_json::from_str::<&RawValue>(data) {
            return Payload::Raw(raw);
        }
    }
    serde_json::from_str::<serde_json::Value>(data)
        .map(Payload::Value)
        .unwrap_or(Payload::Text(data))
}

#[derive(Serialize)]
struct Envelope<'a> {
    channel: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Payload<'a>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    activity: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<&'a ChannelEnd>,
}

impl<'a> Envelope<'a> {
    fn new(channel: &'a str) -> Self {
        Self {
            channel,
            event: None,
            data: None,
            activity: false,
            open: false,
            content_type: None,
            end: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "lowercase")]
pub(crate) enum ChannelEnd {
    Eof,
    Http { status: u16, body: String },
    Error { message: String },
    Capacity { bytes: usize },
}

fn sse(envelope: &Envelope<'_>) -> Bytes {
    let json = serde_json::to_string(envelope).unwrap_or_else(|_| "{}".into());
    Bytes::from(format!("data: {json}\n\n"))
}

pub(crate) fn encode_frame(channel: &str, frame: &SseFrame) -> Bytes {
    let mut envelope = Envelope::new(channel);
    match frame.data.as_deref() {
        Some(data) => {
            envelope.data = Some(payload(data));
            envelope.event = frame.event.as_deref().filter(|event| *event != "message");
        }
        None => envelope.activity = true,
    }
    sse(&envelope)
}

pub(crate) fn encode_open(channel: &str, content_type: Option<&str>) -> Bytes {
    let mut envelope = Envelope::new(channel);
    envelope.open = true;
    envelope.content_type = content_type;
    sse(&envelope)
}

pub(crate) fn encode_end(channel: &str, end: &ChannelEnd) -> Bytes {
    let mut envelope = Envelope::new(channel);
    envelope.end = Some(end);
    sse(&envelope)
}
