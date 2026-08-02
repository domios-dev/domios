//! Multiplexed web output stream (D1 §5): ONE SSE connection carrying every
//! tab's PTY output as tab-tagged frames, replacing the per-tab 25s
//! long-poll. Cursor = per-tab absolute byte offset over the EXISTING
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

use crate::agent::status::TabStatus;
use crate::api::routes::{check_auth, AppState};
use crate::service::ServiceState;

/// Per-tick per-tab byte cap: round-robin fairness so one noisy tab can't
/// starve the others. Leftover bytes are read on the next tick.
const PER_TAB_TICK_CAP: usize = 128 * 1024;
/// Output sweep cadence (matches the retired per-tab SSE precedent).
const TICK: Duration = Duration::from_millis(200);

#[derive(Deserialize)]
pub(crate) struct OutputStreamQuery {
    since: Option<String>,
    tab_id: Option<String>,
}

/// Decoded `?since` cursor: the boot namespace + per-tab resume offsets.
#[derive(Deserialize, Default)]
struct Cursor {
    #[serde(default)]
    boot: String,
    #[serde(default)]
    tabs: HashMap<String, u64>,
}

/// b64url (with or without padding) of `{boot, tabs:{id:offset}}`.
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

/// One tab's read result for a tick: at most one capped data chunk plus an
/// optional under-retention marker. `new_offset` is where the client resumes.
#[derive(Debug, PartialEq)]
struct TabRead {
    truncated_from: Option<u64>,
    output: Vec<u8>,
    new_offset: u64,
}

/// Pure emit planner over a raw `read_since` result. Split out from the buffer
/// read so the fairness cap, truncation marker, and UTF-8-boundary discipline
/// are unit-testable without constructing an `OutputBuffer`.
fn plan_emit(offset: u64, chunk: &[u8], total: u64, cap: usize) -> TabRead {
    if chunk.is_empty() {
        return TabRead {
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
    TabRead {
        truncated_from,
        output: chunk[..take].to_vec(),
        new_offset: base + take as u64,
    }
}

/// Per-tab cursor + whether an `ended` frame was already emitted (emit once).
#[derive(Clone)]
struct Tracked {
    offset: u64,
    ended: bool,
}

/// Seed initial per-tab offsets (blocking buffer read). Absent cursor ⇒ tail
/// all; boot-match ⇒ resume known tabs at their offset and full-replay tabs
/// created during the gap; boot mismatch ⇒ a `boot-changed` frame then tail all
/// (the client full-resyncs via `mux_get_state`).
fn seed_initial(
    svc: &ServiceState,
    cursor: Option<Cursor>,
    tab_id: Option<&str>,
) -> (Option<serde_json::Value>, HashMap<String, Tracked>) {
    let boot = svc.events.boot_id().to_string();
    let live: Vec<(String, String)> = svc
        .mux
        .read()
        .pty_backed_tabs()
        .into_iter()
        .filter(|(id, _)| tab_id.is_none_or(|wanted| wanted == id))
        .collect();
    let buf = svc.output_buf.lock();
    let mut tracked = HashMap::new();
    let mut boot_frame = None;

    let tail = |pty: &str| Tracked {
        offset: buf.total_written(pty),
        ended: false,
    };
    match cursor {
        None => {
            for (tab, pty) in &live {
                tracked.insert(tab.clone(), tail(pty));
            }
        }
        Some(c) if c.boot == boot => {
            for (tab, _pty) in &live {
                let offset = c.tabs.get(tab).copied().unwrap_or(0);
                tracked.insert(
                    tab.clone(),
                    Tracked {
                        offset,
                        ended: false,
                    },
                );
            }
        }
        Some(_) => {
            boot_frame = Some(serde_json::json!({ "boot_changed": true, "boot": boot }));
            for (tab, pty) in &live {
                tracked.insert(tab.clone(), tail(pty));
            }
        }
    }
    (boot_frame, tracked)
}

/// One blocking sweep across every PTY-backed tab. Pure sync — safe for
/// `spawn_blocking`; locks are acquired in order mux > output_buf >
/// agent_tracker and each is dropped before the next, none held across `.await`.
fn sweep(
    svc: &ServiceState,
    tracked: &mut HashMap<String, Tracked>,
    tab_id: Option<&str>,
) -> Vec<serde_json::Value> {
    let live: HashMap<String, String> = svc
        .mux
        .read()
        .pty_backed_tabs()
        .into_iter()
        .filter(|(id, _)| tab_id.is_none_or(|wanted| wanted == id))
        .collect();
    // Disposed tabs (gone from mux) are dropped silently.
    tracked.retain(|tab, _| live.contains_key(tab));
    // Tabs created mid-stream full-replay from the start of the retained ring.
    for tab in live.keys() {
        tracked.entry(tab.clone()).or_insert(Tracked {
            offset: 0,
            ended: false,
        });
    }

    let mut frames = Vec::new();
    let mut silent: Vec<String> = Vec::new();
    {
        let buf = svc.output_buf.lock();
        for (tab, pty) in &live {
            let t = tracked.get_mut(tab).expect("seeded above");
            if t.ended {
                silent.push(tab.clone());
                continue;
            }
            let (chunk, total) = buf.read_since(pty, t.offset);
            let read = plan_emit(t.offset, &chunk, total, PER_TAB_TICK_CAP);
            if let Some(from) = read.truncated_from {
                frames.push(serde_json::json!({ "tab_id": tab, "truncated": true, "from": from }));
            }
            if read.output.is_empty() {
                t.offset = read.new_offset;
                silent.push(tab.clone());
                continue;
            }
            let text = String::from_utf8_lossy(&read.output).to_string();
            frames.push(
                serde_json::json!({ "tab_id": tab, "output": text, "offset": read.new_offset }),
            );
            t.offset = read.new_offset;
        }
    }

    if !silent.is_empty() {
        let tracker = svc.agent_tracker.lock();
        let known: HashSet<String> = tracker.all_ids().into_iter().collect();
        for tab in silent {
            let pty = &live[&tab];
            let t = tracked.get_mut(&tab).expect("seeded above");
            let dead = known.contains(pty) && tracker.get_status(pty) == TabStatus::Dead;
            if dead {
                if !t.ended {
                    frames.push(serde_json::json!({ "tab_id": tab, "ended": true }));
                    t.ended = true;
                }
            } else if t.ended {
                // Respawned into a fresh pty on the same tab — full-replay it.
                t.ended = false;
                t.offset = 0;
            }
        }
    }
    frames
}

/// `GET /api/output/stream?since=<b64url cursor>` — all tabs' PTY output over
/// one connection (D1 §5). Frames: `{tab_id, output, offset}`, plus per-tab
/// `{truncated, from}` on under-retention and `{ended}` on genuine death;
/// `{boot_changed, boot}` once on a boot mismatch. Disposed tabs go silent.
pub async fn output_stream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<OutputStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let cursor = q.since.as_deref().and_then(parse_cursor);
    let tab_id = q.tab_id;

    let stream = async_stream::stream! {
        let svc0 = svc.clone();
        let (boot_frame, mut tracked) =
            match tokio::task::spawn_blocking({
                let tab_id = tab_id.clone();
                move || seed_initial(&svc0, cursor, tab_id.as_deref())
            }).await {
                Ok(v) => v,
                Err(_) => return,
            };
        if let Some(bf) = boot_frame {
            yield Ok(data_frame(bf));
        }
        loop {
            let svc1 = svc.clone();
            let tab_id = tab_id.clone();
            let mut owned = std::mem::take(&mut tracked);
            let result = tokio::task::spawn_blocking(move || {
                let frames = sweep(&svc1, &mut owned, tab_id.as_deref());
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
