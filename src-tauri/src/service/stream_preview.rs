//! Ephemeral chat stream preview: one provider-agnostic [`MotionDecoder`] per
//! pty, fed the already-parsed screen lines from the live output path. Emits a
//! cumulative reconstruction of the in-flight turn so Chat UI can render a
//! streaming block before the canonical JSONL message lands. Telemetry only:
//! never consulted by delivery, the ledger, or state classification.

use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde::Serialize;

use crate::agent::motion_decoder::MotionDecoder;

/// Upper bound on retained frozen rows per turn (memory bound per tab).
const FROZEN_CAP: usize = 400;
/// Rows of frozen history included in each event (cumulative tail, idempotent).
const EMIT_TAIL: usize = 200;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StreamPreviewEvent {
    pub tab_id: String,
    pub turn_seq: u32,
    /// Cumulative frozen turn rows (tail-capped). Replaces, never appends.
    pub frozen: Vec<String>,
    /// Current on-screen turn rows. Replaces, never appends.
    pub live: Vec<String>,
}

#[derive(Default)]
struct PreviewState {
    decoder: MotionDecoder,
    frozen: Vec<String>,
    turn_seq: u32,
    last_fingerprint: u64,
}

fn registry() -> &'static Mutex<HashMap<String, PreviewState>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, PreviewState>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Signal-only turn boundary, fired once per submit transaction when the
/// submit key reaches the write queue.
pub(crate) fn begin_turn(pty_id: &str) {
    let mut reg = registry().lock();
    let state = reg.entry(pty_id.to_string()).or_default();
    state.decoder.begin_turn();
    state.frozen.clear();
    state.last_fingerprint = 0;
}

/// Advance the decoder with freshly parsed screen lines. Returns a change-gated
/// cumulative event when the in-flight turn view differs from the last emit.
pub(crate) fn step(
    pty_id: &str,
    tab_id: &str,
    lines: &[String],
    cursor_row: Option<usize>,
) -> Option<StreamPreviewEvent> {
    let mut reg = registry().lock();
    let state = reg.entry(pty_id.to_string()).or_default();
    let frame = state.decoder.step_with_cursor(lines, cursor_row);
    if frame.turn_seq == 0 {
        return None;
    }
    if frame.turn_seq != state.turn_seq {
        state.turn_seq = frame.turn_seq;
        state.frozen.clear();
    }
    state.frozen.extend(frame.turn_frozen);
    if state.frozen.len() > FROZEN_CAP {
        let excess = state.frozen.len() - FROZEN_CAP;
        state.frozen.drain(..excess);
    }
    let fingerprint = fingerprint(state.turn_seq, &state.frozen, &frame.turn_live);
    if fingerprint == state.last_fingerprint {
        return None;
    }
    state.last_fingerprint = fingerprint;
    let tail_from = state.frozen.len().saturating_sub(EMIT_TAIL);
    Some(StreamPreviewEvent {
        tab_id: tab_id.to_string(),
        turn_seq: state.turn_seq,
        frozen: state.frozen[tail_from..].to_vec(),
        live: frame.turn_live,
    })
}

pub(crate) fn remove(pty_id: &str) {
    registry().lock().remove(pty_id);
}

fn fingerprint(turn_seq: u32, frozen: &[String], live: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(turn_seq);
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for line in frozen.iter().chain(live) {
        feed(line.as_bytes());
    }
    hash
}

#[cfg(test)]
mod tests;
