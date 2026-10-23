use std::time::Duration;

pub(super) async fn wait(client: &reqwest::Client, origin: &str) -> Result<(), String> {
    let endpoint = url::Url::parse(origin)
        .and_then(|origin| origin.join("/api/startup/events"))
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let mut response = client
                .get(endpoint.clone())
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
                .await
                .map_err(|error| error.to_string())?
                .error_for_status()
                .map_err(|error| error.to_string())?;
            if !response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
            {
                return Err("Server does not support startup readiness events".into());
            }
            let mut events = StartupEvents::default();
            while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
                if events.push(&chunk)? {
                    return Ok(());
                }
            }
        }
    })
    .await
    .map_err(|_| "Server startup readiness deadline exceeded".to_string())?
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
                        Some("starting") => false,
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
