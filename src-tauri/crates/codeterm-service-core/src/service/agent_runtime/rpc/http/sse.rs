use super::*;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::Deserializer as _;
use std::io::Read;
use tokio::io::AsyncBufReadExt;

// Match the 4 MiB runtime JSON budget so full-text echoes can carry delivery evidence.
pub(super) const EVENT_LIMIT: usize = MAX_RUNTIME_VALUE_BYTES;
const INLINE_JSON_LIMIT: usize = 256 * 1024;
const EVENT_TYPE_PROBE_LIMIT: u64 = 4096;

pub(super) async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<Option<String>, RuntimeFailure> {
    let mut line = Vec::new();
    loop {
        let bytes = reader
            .fill_buf()
            .await
            .map_err(|_| failure("HTTP process output failed"))?;
        if bytes.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| failure("HTTP process output encoding"))
            };
        }
        let end = bytes.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(bytes.len(), |index| index + 1);
        if line.len().saturating_add(count) > limit {
            return Err(failure("HTTP process output limit"));
        }
        line.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if end.is_some() {
            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| failure("HTTP process output encoding"));
        }
    }
}

#[derive(Default)]
pub(super) struct Decoder {
    line: Vec<u8>,
    data: Vec<u8>,
    event_type: Option<String>,
    size: usize,
    carriage: bool,
    comment: bool,
}

impl Decoder {
    pub fn push(&mut self, byte: u8) -> Result<Option<Vec<u8>>, RuntimeFailure> {
        if self.carriage && byte == b'\n' {
            self.carriage = false;
            return Ok(None);
        }
        self.carriage = byte == b'\r';
        let end_of_line = matches!(byte, b'\r' | b'\n');
        if self.comment || (self.line.is_empty() && byte == b':') {
            self.comment = !end_of_line;
            return Ok(None);
        }
        self.size += 1;
        if self.size > EVENT_LIMIT {
            return Err(RuntimeFailure::SseEventTooLarge {
                event_type: self.event_name(),
                limit_bytes: EVENT_LIMIT,
            });
        }
        if !end_of_line {
            self.line.push(byte);
            return Ok(None);
        }
        if self.line.is_empty() {
            self.size = 0;
            self.event_type = None;
            if self.data.is_empty() {
                return Ok(None);
            }
            self.data.pop();
            return Ok(Some(std::mem::take(&mut self.data)));
        }
        if let Some(value) = field(&self.line, b"data:") {
            self.data.extend_from_slice(value);
            self.data.push(b'\n');
        } else if let Some(value) = field(&self.line, b"event:") {
            self.event_type = std::str::from_utf8(value)
                .ok()
                .filter(|value| valid_event_type(value))
                .map(str::to_owned);
        }
        if self.line.capacity() > INLINE_JSON_LIMIT {
            self.line = Vec::new();
        } else {
            self.line.clear();
        }
        Ok(None)
    }

    fn event_name(&self) -> String {
        if let Some(name) = &self.event_type {
            return name.clone();
        }
        let current_data = field(&self.line, b"data:").unwrap_or_default();
        let prefix = self.data.as_slice().chain(current_data);
        let mut name = None;
        // An oversized event can end mid-JSON. Keep the type as soon as it is read.
        let _ = serde_json::Deserializer::from_reader(prefix.take(EVENT_TYPE_PROBE_LIMIT))
            .deserialize_map(EventType(&mut name));
        name.filter(|value| valid_event_type(value))
            .unwrap_or_else(|| "message".into())
    }
}

fn field<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let value = line.strip_prefix(prefix)?;
    Some(value.strip_prefix(b" ").unwrap_or(value))
}

fn valid_event_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

struct EventType<'a>(&'a mut Option<String>);

impl<'de> Visitor<'de> for EventType<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an SSE JSON event object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == "type" {
                *self.0 = map.next_value()?;
                break;
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(())
    }
}

pub(super) fn parse_event(data: &[u8]) -> Result<Value, RuntimeFailure> {
    serde_json::from_slice(data).map_err(|_| failure("SSE JSON invalid"))
}

pub(super) async fn consume(
    mut response: reqwest::Response,
    client: &Client,
    declaration: &RuntimeHttpSse,
    password: &str,
    pending: &Mutex<requests::Requests>,
) -> Result<(), RuntimeFailure> {
    let mut decoder = Decoder::default();
    while let Ok(Some(chunk)) = response.chunk().await {
        for byte in chunk {
            let Some(data) = decoder.push(byte).map_err(|mut reason| {
                if let RuntimeFailure::SseEventTooLarge { event_type, .. } = &mut reason {
                    *event_type = event_type.replace(password, "[redacted]");
                }
                reason
            })?
            else {
                continue;
            };
            let parse = |data: Vec<u8>, password: &str| -> Result<Value, RuntimeFailure> {
                let mut params = parse_event(&data)?;
                redact_value(&mut params, password);
                Ok(params)
            };
            let params = if data.len() <= INLINE_JSON_LIMIT {
                parse(data, password)?
            } else {
                let password = password.to_owned();
                tokio::task::spawn_blocking(move || parse(data, &password))
                    .await
                    .map_err(|_| failure("SSE event task failed"))??
            };
            if let Some(frame) = pending.lock().frame(client, declaration, params)? {
                client.ingest(WireFrame::Event(frame));
            }
        }
    }
    Ok(())
}
