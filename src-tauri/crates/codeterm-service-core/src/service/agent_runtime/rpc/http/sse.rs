use super::*;

const EVENT_LIMIT: usize = 256 * 1024;

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
    size: usize,
    carriage: bool,
}

impl Decoder {
    pub fn push(&mut self, byte: u8) -> Result<Option<Value>, RuntimeFailure> {
        if self.carriage && byte == b'\n' {
            self.carriage = false;
            return Ok(None);
        }
        self.carriage = byte == b'\r';
        self.size += 1;
        if self.size > EVENT_LIMIT {
            return Err(failure("SSE event limit"));
        }
        if !matches!(byte, b'\r' | b'\n') {
            self.line.push(byte);
            return Ok(None);
        }
        if self.line.is_empty() {
            self.size = 0;
            if self.data.is_empty() {
                return Ok(None);
            }
            self.data.pop();
            let data = std::mem::take(&mut self.data);
            return serde_json::from_slice(&data)
                .map(Some)
                .map_err(|_| failure("SSE JSON invalid"));
        }
        if let Some(value) = self.line.strip_prefix(b"data:") {
            let value = value.strip_prefix(b" ").unwrap_or(value);
            self.data.extend_from_slice(value);
            self.data.push(b'\n');
        }
        self.line.clear();
        Ok(None)
    }
}

pub(super) async fn consume(
    mut response: reqwest::Response,
    client: &Client,
    declaration: &RuntimeHttpSse,
    password: &str,
    pending: &Mutex<requests::Requests>,
) -> Result<(), RuntimeFailure> {
    let mut decoder = Decoder::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("SSE disconnected; runtime broken"))?
    {
        for byte in chunk {
            if let Some(mut params) = decoder.push(byte)? {
                redact_value(&mut params, password);
                if let Some(frame) = pending.lock().frame(client, declaration, params)? {
                    client.ingest(WireFrame::Event(frame));
                }
            }
        }
    }
    Err(failure("SSE ended; runtime broken"))
}
