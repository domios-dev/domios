use std::time::Duration;
use std::{fmt, future::Future};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadinessFailure {
    Transport(String),
    Protocol(String),
    EndOfStream,
    Deadline,
}

impl fmt::Display for ReadinessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(reason) | Self::Protocol(reason) => formatter.write_str(reason),
            Self::EndOfStream => {
                formatter.write_str("Server startup stream ended before readiness")
            }
            Self::Deadline => formatter.write_str("Server startup readiness deadline exceeded"),
        }
    }
}

trait StartupBody {
    fn chunk(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, ReadinessFailure>> + Send;
}

impl StartupBody for reqwest::Response {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, ReadinessFailure> {
        reqwest::Response::chunk(self)
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|error| ReadinessFailure::Transport(error.to_string()))
    }
}

async fn observe<B: StartupBody>(
    request: impl Future<Output = Result<B, ReadinessFailure>>,
) -> Result<(), ReadinessFailure> {
    let mut response = request.await?;
    let mut events = StartupEvents::default();
    while let Some(chunk) = response.chunk().await? {
        if events.push(&chunk).map_err(ReadinessFailure::Protocol)? {
            return Ok(());
        }
    }
    Err(ReadinessFailure::EndOfStream)
}

pub(super) async fn wait(client: &reqwest::Client, origin: &str) -> Result<(), ReadinessFailure> {
    let endpoint = url::Url::parse(origin)
        .and_then(|origin| origin.join("/api/startup/events"))
        .map_err(|error| ReadinessFailure::Protocol(error.to_string()))?;
    let request = async {
        let response = client
            .get(endpoint)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| ReadinessFailure::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| ReadinessFailure::Transport(error.to_string()))?;
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
        {
            return Err(ReadinessFailure::Protocol(
                "Server does not support startup readiness events".into(),
            ));
        }
        Ok(response)
    };
    tokio::time::timeout(Duration::from_secs(30), observe(request))
        .await
        .map_err(|_| ReadinessFailure::Deadline)?
}

#[derive(Default)]
struct StartupEvents {
    line: Vec<u8>,
    event: String,
    data: String,
    bytes: usize,
}

impl StartupEvents {
    fn push(&mut self, chunk: &[u8]) -> Result<bool, String> {
        for byte in chunk {
            self.bytes += 1;
            if self.bytes > 64 * 1024 {
                return Err("Server startup event exceeds capacity".into());
            }
            if *byte != b'\n' {
                self.line.push(*byte);
                continue;
            }
            let line = std::str::from_utf8(&self.line)
                .map_err(|error| error.to_string())?
                .trim_end_matches('\r');
            if line.is_empty() {
                let ready = if self.event == "startup-state" {
                    let snapshot: serde_json::Value =
                        serde_json::from_str(&self.data).map_err(|error| error.to_string())?;
                    match snapshot["state"].as_str() {
                        Some("ready") => true,
                        Some("starting") => snapshot["desktop_ready"].as_bool() == Some(true),
                        Some("failed") => return Err("Server initialization failed".into()),
                        _ => return Err("Server startup event has no valid state".into()),
                    }
                } else {
                    false
                };
                self.event.clear();
                self.data.clear();
                self.bytes = 0;
                self.line.clear();
                if ready {
                    return Ok(true);
                }
                continue;
            }
            if let Some((field, value)) = line.split_once(':') {
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => self.event = value.to_string(),
                    "data" => {
                        self.data.push_str(value);
                        self.data.push('\n');
                    }
                    _ => {}
                }
            }
            self.line.clear();
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
