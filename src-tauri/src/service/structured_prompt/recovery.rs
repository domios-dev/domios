use std::time::Duration;

pub(super) const HEARTBEAT_SILENCE: Duration = Duration::from_secs(45);
pub(super) const CONNECT_DEADLINE: Duration = Duration::from_secs(10);
pub(super) const RECONCILE_DEADLINE: Duration = Duration::from_secs(30);
const MAX_OUTAGE_FAILURES: usize = 8;

pub(super) fn heartbeat_deadline(last_frame: tokio::time::Instant) -> tokio::time::Instant {
    last_frame + HEARTBEAT_SILENCE
}

#[derive(Default)]
pub(super) struct Recovery {
    failures: usize,
    pub(super) ever_healthy: bool,
}

impl Recovery {
    pub(super) fn healthy(&mut self) {
        self.ever_healthy = true;
        self.retry_after_event();
    }

    pub(super) fn retry_after_event(&mut self) {
        self.failures = 0;
    }

    pub(super) fn failed(&mut self) -> bool {
        self.failures = self.failures.saturating_add(1);
        self.failures >= MAX_OUTAGE_FAILURES
    }
}

#[derive(Default)]
pub(super) struct Frames {
    pending: Vec<u8>,
    data: Vec<String>,
}

impl Frames {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<serde_json::Value>, String> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > 4 * 1024 * 1024 {
            return Err("structured event exceeds 4 MiB".into());
        }
        let mut frames = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "structured event is not UTF-8")?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    if let Ok(frame) = serde_json::from_str(&self.data.join("\n")) {
                        frames.push(frame);
                    }
                    self.data.clear();
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
                if self.data.iter().map(String::len).sum::<usize>() > 4 * 1024 * 1024 {
                    return Err("structured event exceeds 4 MiB".into());
                }
            }
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_stream_resets_each_outage_without_a_lifetime_cap() {
        let mut recovery = Recovery::default();
        for _ in 0..1_000 {
            for _ in 0..MAX_OUTAGE_FAILURES - 1 {
                assert!(!recovery.failed());
            }
            recovery.healthy();
        }
        for _ in 0..MAX_OUTAGE_FAILURES - 1 {
            assert!(!recovery.failed());
        }
        assert!(recovery.failed());
        recovery.healthy();
        assert!(!recovery.failed());
    }

    #[test]
    fn sse_frames_follow_boundaries_across_chunks_and_crlf() {
        let mut frames = Frames::default();
        assert!(frames.push(b"data: {\"type\":").unwrap().is_empty());
        assert!(frames
            .push(b"\"server.heartbeat\"}\r\n")
            .unwrap()
            .is_empty());
        assert_eq!(
            frames.push(b"\r\n: comment\n\ndata: {}\n\n").unwrap(),
            vec![
                serde_json::json!({"type":"server.heartbeat"}),
                serde_json::json!({})
            ]
        );
    }

    #[test]
    fn heartbeat_watchdog_fails_only_at_the_silence_deadline() {
        let frame = tokio::time::Instant::from_std(std::time::Instant::now());
        let deadline = heartbeat_deadline(frame);
        assert!(frame + Duration::from_secs(44) < deadline);
        assert_eq!(frame + Duration::from_secs(45), deadline);
        assert!(frame + Duration::from_secs(46) > deadline);
        assert_eq!(
            heartbeat_deadline(frame + Duration::from_secs(40)),
            frame + Duration::from_secs(85)
        );
    }
}
