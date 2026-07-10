//! Multiplexed web output stream (D1 §5): ONE SSE connection carrying every
//! pane's PTY output as pane-tagged frames, replacing the per-pane 25s
//! long-poll. Cursor = per-pane absolute byte offset over the EXISTING
//! `OutputBuffer` (the replay ring); `boot_id` is shared with the events stream
//! so a daemon restart invalidates both cursors consistently.

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use base64::Engine;
use futures_core::Stream;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::agent::status::PaneStatus;
use crate::api::routes::{check_auth, AppState};
use crate::service::ServiceState;

/// Per-tick per-pane byte cap: round-robin fairness so one noisy pane can't
/// starve the others. Leftover bytes are read on the next tick.
const PER_PANE_TICK_CAP: usize = 128 * 1024;
/// Output sweep cadence (matches the retired per-pane SSE precedent).
const TICK: Duration = Duration::from_millis(200);

#[derive(Deserialize)]
pub(crate) struct OutputStreamQuery {
    since: Option<String>,
}

/// Decoded `?since` cursor: the boot namespace + per-pane resume offsets.
#[derive(Deserialize, Default)]
struct Cursor {
    #[serde(default)]
    boot: String,
    #[serde(default)]
    panes: HashMap<String, u64>,
}

/// b64url (with or without padding) of `{boot, panes:{id:offset}}`.
fn parse_cursor(raw: &str) -> Option<Cursor> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw.as_bytes()))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn data_frame(v: serde_json::Value) -> Event {
    Event::default().data(v.to_string())
}

/// One pane's read result for a tick: at most one capped data chunk plus an
/// optional under-retention marker. `new_offset` is where the client resumes.
#[derive(Debug, PartialEq)]
struct PaneRead {
    truncated_from: Option<u64>,
    output: Vec<u8>,
    new_offset: u64,
}

/// Pure emit planner over a raw `read_since` result. Split out from the buffer
/// read so the fairness cap, truncation marker, and UTF-8-boundary discipline
/// are unit-testable without constructing an `OutputBuffer`.
fn plan_emit(offset: u64, chunk: &[u8], total: u64, cap: usize) -> PaneRead {
    if chunk.is_empty() {
        return PaneRead {
            truncated_from: None,
            output: Vec::new(),
            new_offset: total,
        };
    }
    // first byte's absolute offset is therefore `total - chunk.len()`.
    let base = total - chunk.len() as u64;
    // Fewer bytes than the gap from the cursor means the front was evicted.
    let truncated_from = if total - offset > chunk.len() as u64 {
        Some(base)
    } else {
        None
    };
    let mut take = chunk.len().min(cap);
    // would corrupt it via `from_utf8_lossy` here and on the next tick.
    while take > 0 && take < chunk.len() && (chunk[take] & 0b1100_0000) == 0b1000_0000 {
        take -= 1;
    }
    if take == 0 {
        take = chunk.len().min(cap); // guarantee forward progress
    }
    PaneRead {
        truncated_from,
        output: chunk[..take].to_vec(),
        new_offset: base + take as u64,
    }
}

/// Per-pane cursor + whether an `ended` frame was already emitted (emit once).
#[derive(Clone)]
struct Tracked {
    offset: u64,
    ended: bool,
}

/// Seed initial per-pane offsets (blocking buffer read). Absent cursor ⇒ tail
/// all; boot-match ⇒ resume known panes at their offset and full-replay panes
/// created during the gap; boot mismatch ⇒ a `boot-changed` frame then tail all
/// (the client full-resyncs via `mux_get_state`).
fn seed_initial(
    svc: &ServiceState,
    cursor: Option<Cursor>,
) -> (Option<serde_json::Value>, HashMap<String, Tracked>) {
    let boot = svc.events.boot_id().to_string();
    let live: Vec<(String, String)> = svc.mux.read().pty_backed_panes();
    let buf = svc.output_buf.lock();
    let mut tracked = HashMap::new();
    let mut boot_frame = None;

    let tail = |pty: &str| Tracked {
        offset: buf.total_written(pty),
        ended: false,
    };
    match cursor {
        None => {
            for (pane, pty) in &live {
                tracked.insert(pane.clone(), tail(pty));
            }
        }
        Some(c) if c.boot == boot => {
            for (pane, _pty) in &live {
                let offset = c.panes.get(pane).copied().unwrap_or(0);
                tracked.insert(
                    pane.clone(),
                    Tracked {
                        offset,
                        ended: false,
                    },
                );
            }
        }
        Some(_) => {
            boot_frame = Some(serde_json::json!({ "boot_changed": true, "boot": boot }));
            for (pane, pty) in &live {
                tracked.insert(pane.clone(), tail(pty));
            }
        }
    }
    (boot_frame, tracked)
}

/// One blocking sweep across every PTY-backed pane. Pure sync — safe for
/// `spawn_blocking`; locks are acquired in order mux > output_buf >
/// agent_tracker and each is dropped before the next, none held across `.await`.
fn sweep(svc: &ServiceState, tracked: &mut HashMap<String, Tracked>) -> Vec<serde_json::Value> {
    let live: HashMap<String, String> = svc.mux.read().pty_backed_panes().into_iter().collect();
    // Disposed panes (gone from mux) are dropped silently.
    tracked.retain(|pane, _| live.contains_key(pane));
    // Panes created mid-stream full-replay from the start of the retained ring.
    for pane in live.keys() {
        tracked.entry(pane.clone()).or_insert(Tracked {
            offset: 0,
            ended: false,
        });
    }

    let mut frames = Vec::new();
    let mut silent: Vec<String> = Vec::new();
    {
        let buf = svc.output_buf.lock();
        for (pane, pty) in &live {
            let t = tracked.get_mut(pane).expect("seeded above");
            if t.ended {
                silent.push(pane.clone());
                continue;
            }
            let (chunk, total) = buf.read_since(pty, t.offset);
            let read = plan_emit(t.offset, &chunk, total, PER_PANE_TICK_CAP);
            if let Some(from) = read.truncated_from {
                frames
                    .push(serde_json::json!({ "pane_id": pane, "truncated": true, "from": from }));
            }
            if read.output.is_empty() {
                t.offset = read.new_offset;
                silent.push(pane.clone());
                continue;
            }
            let text = String::from_utf8_lossy(&read.output).to_string();
            frames.push(
                serde_json::json!({ "pane_id": pane, "output": text, "offset": read.new_offset }),
            );
            t.offset = read.new_offset;
        }
    }

    if !silent.is_empty() {
        let tracker = svc.agent_tracker.lock();
        let known: HashSet<String> = tracker.all_ids().into_iter().collect();
        for pane in silent {
            let pty = &live[&pane];
            let t = tracked.get_mut(&pane).expect("seeded above");
            let dead = known.contains(pty) && tracker.get_status(pty) == PaneStatus::Dead;
            if dead {
                if !t.ended {
                    frames.push(serde_json::json!({ "pane_id": pane, "ended": true }));
                    t.ended = true;
                }
            } else if t.ended {
                // Respawned into a fresh pty on the same pane — full-replay it.
                t.ended = false;
                t.offset = 0;
            }
        }
    }
    frames
}

/// `GET /api/output/stream?since=<b64url cursor>` — all panes' PTY output over
/// one connection (D1 §5). Frames: `{pane_id, output, offset}`, plus per-pane
/// `{truncated, from}` on under-retention and `{ended}` on genuine death;
/// `{boot_changed, boot}` once on a boot mismatch. Disposed panes go silent.
pub async fn output_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<OutputStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let cursor = q.since.as_deref().and_then(parse_cursor);

    let stream = async_stream::stream! {
        let svc0 = svc.clone();
        let (boot_frame, mut tracked) =
            match tokio::task::spawn_blocking(move || seed_initial(&svc0, cursor)).await {
                Ok(v) => v,
                Err(_) => return,
            };
        if let Some(bf) = boot_frame {
            yield Ok(data_frame(bf));
        }
        loop {
            let svc1 = svc.clone();
            let mut owned = std::mem::take(&mut tracked);
            let result = tokio::task::spawn_blocking(move || {
                let frames = sweep(&svc1, &mut owned);
                (frames, owned)
            })
            .await;
            let (frames, owned) = match result {
                Ok(v) => v,
                Err(_) => return,
            };
            tracked = owned;
            for f in frames {
                yield Ok(data_frame(f));
            }
            tokio::time::sleep(TICK).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests;
