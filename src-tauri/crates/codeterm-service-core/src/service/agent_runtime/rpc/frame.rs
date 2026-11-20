use super::{RpcId, RuntimeFailure};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub(super) const INBOUND_FRAME_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(super) enum Inbound {
    Text(String),
    Oversized(Option<RpcId>),
}

pub(super) async fn read_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Inbound>, RuntimeFailure> {
    let mut frame = Vec::new();
    let mut envelope: Option<Envelope> = None;
    loop {
        let bytes = reader
            .fill_buf()
            .await
            .map_err(|error| RuntimeFailure::PipeFailure {
                stream: "stdout".into(),
                reason: error.to_string(),
            })?;
        if bytes.is_empty() {
            break;
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |end| end + 1);
        if envelope.is_none() && frame.len() + count > INBOUND_FRAME_LIMIT {
            let mut scan = Envelope::default();
            scan.push(&std::mem::take(&mut frame));
            envelope = Some(scan);
        }
        if let Some(scan) = envelope.as_mut() {
            scan.push(&bytes[..count]);
        } else {
            let needed = frame.len() + count;
            if needed > frame.capacity() {
                let capacity = (frame.capacity() * 2).max(needed).min(INBOUND_FRAME_LIMIT);
                frame.reserve_exact(capacity - frame.len());
            }
            frame.extend_from_slice(&bytes[..count]);
        }
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    if let Some(scan) = envelope {
        return Ok(Some(Inbound::Oversized(scan.response_id())));
    }
    if frame.is_empty() {
        return Ok(None);
    }
    String::from_utf8(frame)
        .map(|text| Some(Inbound::Text(text)))
        .map_err(|error| RuntimeFailure::InvalidFrame(error.to_string()))
}

// Scan top-level field boundaries only. JSON-RPC permits the ID after the payload.
#[derive(Default)]
struct Envelope {
    started: bool,
    complete: bool,
    invalid: bool,
    depth: usize,
    string: bool,
    escaped: bool,
    key: Token,
    value: Token,
    field: Option<Field>,
    seen: u8,
    id: Option<i64>,
}

#[derive(Clone, Copy)]
enum Field {
    Other = 0,
    Id = 1,
    Version = 2,
    Method = 4,
    Result = 8,
    Error = 16,
}

#[derive(Default)]
struct Token {
    bytes: Vec<u8>,
    full: bool,
}

impl Token {
    fn push(&mut self, byte: u8, in_string: bool) {
        // JSON whitespace outside tokens needs at most one byte per run.
        if !in_string && byte.is_ascii_whitespace() {
            if self.bytes.is_empty() || self.bytes.last() == Some(&b' ') {
                return;
            }
            self.store(b' ');
        } else {
            self.store(byte);
        }
    }

    fn store(&mut self, byte: u8) {
        if self.bytes.len() < 128 {
            self.bytes.push(byte);
        } else {
            self.full = true;
        }
    }

    fn parse<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        if self.full {
            return None;
        }
        serde_json::from_slice(&self.bytes).ok()
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.full = false;
    }
}

impl Envelope {
    fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if self.invalid {
                return;
            }
            self.push_byte(byte);
        }
    }

    fn push_byte(&mut self, byte: u8) {
        if !self.started || self.complete {
            if byte.is_ascii_whitespace() {
                return;
            }
            self.invalid = self.complete || byte != b'{';
            self.started = true;
            return;
        }
        if !self.string && self.depth == 0 {
            match byte {
                b':' if self.field.is_none() => {
                    self.field = Some(match self.key.parse::<String>().as_deref() {
                        Some("id") => Field::Id,
                        Some("jsonrpc") => Field::Version,
                        Some("method") => Field::Method,
                        Some("result") => Field::Result,
                        Some("error") => Field::Error,
                        _ => Field::Other,
                    });
                    return;
                }
                b',' | b'}' => {
                    self.finish_field();
                    self.complete = byte == b'}';
                    return;
                }
                _ => {}
            }
        }
        match self.field {
            None => self.key.push(byte, self.string),
            Some(Field::Id | Field::Version) => self.value.push(byte, self.string),
            _ => {}
        }
        if self.string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.string = false;
            }
        } else {
            match byte {
                b'"' => self.string = true,
                b'{' | b'[' => self.depth += 1,
                b'}' | b']' if self.depth > 0 => self.depth -= 1,
                b']' => self.invalid = true,
                _ => {}
            }
        }
    }

    fn finish_field(&mut self) {
        let Some(field) = self.field.take() else {
            self.invalid = true;
            return;
        };
        let bit = field as u8;
        self.invalid |= self.seen & bit != 0;
        self.seen |= bit;
        match field {
            Field::Id => self.id = self.value.parse(),
            Field::Version => {
                self.invalid |= self.value.parse::<String>().as_deref() != Some("2.0")
            }
            _ => {}
        }
        self.key.clear();
        self.value.clear();
    }

    fn response_id(&self) -> Option<RpcId> {
        let kind = self.seen & (Field::Method as u8 | Field::Result as u8 | Field::Error as u8);
        if self.invalid
            || !self.complete
            || (kind != Field::Result as u8 && kind != Field::Error as u8)
        {
            return None;
        }
        self.id
            .filter(|id| id.unsigned_abs() <= 9_007_199_254_740_991)
            .map(RpcId::Number)
    }
}
