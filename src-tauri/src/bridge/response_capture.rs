//! Response capture — polls agent output after sending a message (app-only).

use crate::agent::status::TabStatus;
use crate::{AgentTrackerState, OutputBufferState};
use std::time::{Duration, Instant};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub fn capture_response(
    pty_id: &str,
    output_buf: &OutputBufferState,
    tracker: &AgentTrackerState,
) -> Result<String, String> {
    let offset = output_buf
        .lock()
        .get_full(pty_id)
        .map(|b| b.len())
        .unwrap_or(0);

    let deadline = Instant::now() + RESPONSE_TIMEOUT;
    let mut saw_working = false;

    loop {
        if Instant::now() > deadline {
            if saw_working {
                break;
            }
            return Err("Response timeout".to_string());
        }
        let status = tracker.lock().get_status(pty_id);
        match status {
            TabStatus::Working => {
                saw_working = true;
            }
            TabStatus::Idle if saw_working => {
                std::thread::sleep(Duration::from_millis(300));
                break;
            }
            TabStatus::Dead => return Err("Agent died".to_string()),
            _ => {}
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    let raw = output_buf
        .lock()
        .get_full(pty_id)
        .map(|b| {
            if offset < b.len() {
                String::from_utf8_lossy(&b[offset..]).to_string()
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    if raw.is_empty() {
        return Err("No response".to_string());
    }
    Ok(codeterm_bridge::bridge::response::strip_ansi(&raw))
}
