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

use crate::agent::status::TabStatus;
use crate::api::routes::{check_auth, AppState};
use crate::api::stream_diagnostics::{configured_level, StreamDiagnostics, StreamTabStart};
use crate::service::ServiceState;

/// Per-sweep per-tab byte cap: round-robin fairness so one noisy tab can't
/// starve the others. Leftover bytes are read on the next sweep.
const PER_TAB_SWEEP_CAP: usize = 128 * 1024;

#[derive(Deserialize)]
pub(crate) struct OutputStreamQuery {
    since: Option<String>,
    tab_id: Option<String>,
}

/// Decoded `?since` cursor: the boot namespace + per-tab resume offsets.
#[derive(Clone, Deserialize, Default)]
pub(crate) struct Cursor {
    #[serde(default)]
    pub(crate) boot: String,
    #[serde(default)]
    pub(crate) tabs: HashMap<String, u64>,
}

/// b64url (with or without padding) of `{boot, tabs:{id:offset}}`.
pub(crate) fn parse_cursor(raw: &str) -> Option<Cursor> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw.as_bytes()))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn data_frame(v: serde_json::Value) -> Event {
    Event::default().data(v.to_string())
}

fn build_stream_diagnostics(
    level: Option<log::Level>,
    session_id: Option<String>,
    scope: String,
    cursor_boot: Option<String>,
    cursor_offset: Option<u64>,
) -> Option<StreamDiagnostics> {
    level.zip(session_id).map(|(level, session_id)| {
        StreamDiagnostics::new(level, "peer", session_id, scope, cursor_boot, cursor_offset)
    })
}

/// One tab's read result for a sweep: at most one capped data chunk plus an
/// optional under-retention marker. `new_offset` is where the client resumes.
#[derive(Debug, PartialEq)]
struct TabRead {
    truncated_from: Option<u64>,
    output: Vec<u8>,
    new_offset: u64,
    has_more: bool,
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
            has_more: false,
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
    // would corrupt it via `from_utf8_lossy` here and on the next sweep.
    while take > 0 && take < chunk.len() && (chunk[take] & 0b1100_0000) == 0b1000_0000 {
        take -= 1;
    }
    if take == 0 {
        take = chunk.len().min(cap); // guarantee forward progress
    }
    let new_offset = base + take as u64;
    TabRead {
        truncated_from,
        output: chunk[..take].to_vec(),
        new_offset,
        has_more: new_offset < total,
    }
}

/// Per-tab cursor + whether an `ended` frame was already emitted (emit once).
#[derive(Clone)]
struct Tracked {
    pty_id: String,
    offset: u64,
    ended: bool,
}

fn tab_is_in_scope(candidate: &str, requested: Option<&str>) -> bool {
    requested.is_none_or(|wanted| wanted == candidate)
}

/// Seed initial per-tab offsets (blocking buffer read). A tab-scoped stream
/// starts at zero; boot-match resumes known tabs and full-replays new tabs
/// created during the gap; boot mismatch ⇒ a `boot-changed` frame then tail all
/// (the client full-resyncs via `mux_get_state`).
fn seed_initial(
    svc: &ServiceState,
    cursor: Option<Cursor>,
    tab_id: Option<&str>,
) -> (
    Option<serde_json::Value>,
    HashMap<String, Tracked>,
    Vec<StreamTabStart>,
) {
    let boot = svc.events.boot_id().to_string();
    let live: Vec<(String, String)> = svc
        .mux
        .read()
        .pty_backed_tabs()
        .into_iter()
        .filter(|(id, _)| tab_is_in_scope(id, tab_id))
        .collect();
    let buf = svc.output_buf.lock();
    let initial_tabs = live
        .iter()
        .map(|(tab, pty)| {
            let total_written = buf.total_written(pty);
            let retained_from = total_written.saturating_sub(
                buf.get_full(pty)
                    .map(|bytes| bytes.len() as u64)
                    .unwrap_or(0),
            );
            InitialTab {
                tab_id: tab.clone(),
                pty_id: pty.clone(),
                total_written,
                retained_from,
            }
        })
        .collect();
    drop(buf);
    seed_initial_state(boot, cursor, tab_id, initial_tabs)
}

struct InitialTab {
    tab_id: String,
    pty_id: String,
    total_written: u64,
    retained_from: u64,
}

fn seed_initial_state(
    boot: String,
    cursor: Option<Cursor>,
    tab_id: Option<&str>,
    live: Vec<InitialTab>,
) -> (
    Option<serde_json::Value>,
    HashMap<String, Tracked>,
    Vec<StreamTabStart>,
) {
    let mut tracked = HashMap::new();
    let mut boot_frame = None;
    let boot_changed = cursor.as_ref().is_some_and(|value| value.boot != boot);
    if tab_id.is_some() && cursor.is_none() {
        boot_frame = Some(serde_json::json!({ "boot": boot }));
    } else if boot_changed {
        boot_frame = Some(serde_json::json!({ "boot_changed": true, "boot": boot }));
    }

    let mut starts = Vec::with_capacity(live.len());
    for entry in live {
        let InitialTab {
            tab_id: tab,
            pty_id: pty,
            total_written,
            retained_from,
        } = entry;
        let cursor_offset = cursor
            .as_ref()
            .and_then(|value| value.tabs.get(&tab))
            .copied();
        let (resume_offset, replaying) = if boot_changed || (cursor.is_none() && tab_id.is_none()) {
            (total_written, false)
        } else {
            (cursor_offset.unwrap_or(0), true)
        };
        let replay_truncated_from =
            (replaying && cursor_offset.unwrap_or(0) < retained_from).then_some(retained_from);
        tracked.insert(
            tab.clone(),
            Tracked {
                pty_id: pty.clone(),
                offset: resume_offset,
                ended: false,
            },
        );
        starts.push(StreamTabStart {
            tab_id: tab.clone(),
            pty_id: Some(pty),
            cursor_boot: cursor.as_ref().map(|value| value.boot.clone()),
            cursor_offset,
            total_written: Some(total_written),
            resume_offset: Some(resume_offset),
            replay_truncated: Some(replay_truncated_from.is_some()),
            replay_truncated_from,
            boot_changed,
        });
    }
    (boot_frame, tracked, starts)
}

fn selected_tabs(
    live: &HashMap<String, String>,
    dirty_pty_ids: &HashSet<String>,
    changed_tabs: &HashSet<String>,
    read_all: bool,
) -> Vec<(String, String)> {
    live.iter()
        .filter(|(tab, pty)| {
            read_all || dirty_pty_ids.contains(pty.as_str()) || changed_tabs.contains(*tab)
        })
        .map(|(tab, pty)| (tab.clone(), pty.clone()))
        .collect()
}

/// One blocking sweep over changed PTYs. Pure sync — safe for
/// `spawn_blocking`; locks are acquired in order mux > output_buf >
/// agent_tracker and each is dropped before the next, none held across `.await`.
fn sweep(
    svc: &ServiceState,
    tracked: &mut HashMap<String, Tracked>,
    tab_id: Option<&str>,
    dirty_pty_ids: &HashSet<String>,
    read_all: bool,
) -> (Vec<serde_json::Value>, Vec<StreamTabStart>, Vec<String>) {
    let live: HashMap<String, String> = svc
        .mux
        .read()
        .pty_backed_tabs()
        .into_iter()
        .filter(|(id, _)| tab_is_in_scope(id, tab_id))
        .collect();
    // Disposed tabs (gone from mux) are dropped silently.
    tracked.retain(|tab, _| live.contains_key(tab));
    // Tabs created mid-stream full-replay from the start of the retained ring.
    let new_tabs: Vec<String> = live
        .keys()
        .filter(|tab| {
            tracked
                .get(*tab)
                .is_none_or(|existing| existing.pty_id.as_str() != live[*tab].as_str())
        })
        .cloned()
        .collect();
    for tab in &new_tabs {
        tracked.insert(
            tab.clone(),
            Tracked {
                pty_id: live[tab].clone(),
                offset: 0,
                ended: false,
            },
        );
    }

    let mut frames = Vec::new();
    let mut starts = Vec::with_capacity(new_tabs.len());
    let mut silent: Vec<String> = Vec::new();
    let mut more_pty_ids = Vec::new();
    let boot = svc.events.boot_id().to_string();
    let changed_tab_ids: HashSet<String> = new_tabs.iter().cloned().collect();
    let selected = selected_tabs(&live, dirty_pty_ids, &changed_tab_ids, read_all);
    {
        let buf = svc.output_buf.lock();
        for tab in &new_tabs {
            let pty = &live[tab];
            let total_written = buf.total_written(pty);
            let retained_from = total_written.saturating_sub(
                buf.get_full(pty)
                    .map(|bytes| bytes.len() as u64)
                    .unwrap_or(0),
            );
            let replay_truncated_from = (retained_from > 0).then_some(retained_from);
            starts.push(StreamTabStart {
                tab_id: tab.clone(),
                pty_id: Some(pty.clone()),
                cursor_boot: Some(boot.clone()),
                cursor_offset: Some(0),
                total_written: Some(total_written),
                resume_offset: Some(0),
                replay_truncated: Some(replay_truncated_from.is_some()),
                replay_truncated_from,
                boot_changed: false,
            });
        }
        for (tab, pty) in &selected {
            let t = tracked.entry(tab.clone()).or_insert_with(|| Tracked {
                pty_id: pty.clone(),
                offset: 0,
                ended: false,
            });
            if t.ended {
                silent.push(tab.clone());
                continue;
            }
            let (chunk, total) = buf.read_since(pty, t.offset);
            let read = plan_emit(t.offset, &chunk, total, PER_TAB_SWEEP_CAP);
            if read.has_more {
                more_pty_ids.push(pty.clone());
            }
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
            let Some(pty) = live.get(&tab) else {
                continue;
            };
            let t = tracked.entry(tab.clone()).or_insert_with(|| Tracked {
                pty_id: pty.clone(),
                offset: 0,
                ended: false,
            });
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
    (frames, starts, more_pty_ids)
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
    let scope = tab_id.clone().unwrap_or_else(|| "*".into());
    let cursor_boot = cursor.as_ref().map(|value| value.boot.clone());
    let cursor_offset = tab_id
        .as_deref()
        .and_then(|tab| cursor.as_ref()?.tabs.get(tab))
        .copied();
    let diagnostic_level = configured_level(state.config.lock().diagnostics.terminal_transport);
    let session_id = diagnostic_level.map(|_| uuid::Uuid::new_v4().simple().to_string());

    let stream = async_stream::stream! {
        let mut activity = svc.output_stream_activity.subscribe();
        let mut dirty_pty_ids = HashSet::new();
        let mut read_all = true;
        let mut refresh_tabs = true;
        let mut diagnostics = build_stream_diagnostics(
            diagnostic_level,
            session_id,
            scope,
            cursor_boot,
            cursor_offset,
        );
        let svc0 = svc.clone();
        let (boot_frame, mut tracked, starts) =
            match tokio::task::spawn_blocking({
                let tab_id = tab_id.clone();
                move || seed_initial(&svc0, cursor, tab_id.as_deref())
            }).await {
                Ok(v) => v,
                Err(_) => {
                    if let Some(diagnostics) = diagnostics.as_mut() {
                        diagnostics.finish("seed_error");
                    }
                    return;
                },
            };
        for start in starts {
            if let Some(diagnostics) = diagnostics.as_mut() {
                diagnostics.attach_tab(start);
            }
        }
        if let Some(bf) = boot_frame {
            if let Some(diagnostics) = diagnostics.as_mut() {
                diagnostics.observe_value(&bf);
            }
            yield Ok(data_frame(bf));
        }
        loop {
            if !read_all && !refresh_tabs && dirty_pty_ids.is_empty() {
                match activity.recv().await {
                    Ok(crate::service::output_stream_activity::OutputStreamActivity::Pty(pty)) => {
                        dirty_pty_ids.insert(pty);
                    }
                    Ok(crate::service::output_stream_activity::OutputStreamActivity::Mux) => {
                        refresh_tabs = true;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        read_all = true;
                        refresh_tabs = true;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        if let Some(diagnostics) = diagnostics.as_mut() {
                            diagnostics.finish("activity_closed");
                        }
                        return;
                    }
                }
                loop {
                    match activity.try_recv() {
                        Ok(crate::service::output_stream_activity::OutputStreamActivity::Pty(pty)) => {
                            dirty_pty_ids.insert(pty);
                        }
                        Ok(crate::service::output_stream_activity::OutputStreamActivity::Mux) => {
                            refresh_tabs = true;
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                            read_all = true;
                            refresh_tabs = true;
                            break;
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                            if let Some(diagnostics) = diagnostics.as_mut() {
                                diagnostics.finish("activity_closed");
                            }
                            return;
                        }
                    }
                }
            }
            let dirty_for_sweep = std::mem::take(&mut dirty_pty_ids);
            let read_all_for_sweep = std::mem::replace(&mut read_all, false);
            refresh_tabs = false;
            let svc1 = svc.clone();
            let tab_id = tab_id.clone();
            let mut owned = std::mem::take(&mut tracked);
            let result = tokio::task::spawn_blocking(move || {
                let (frames, starts, more_pty_ids) = sweep(
                    &svc1,
                    &mut owned,
                    tab_id.as_deref(),
                    &dirty_for_sweep,
                    read_all_for_sweep,
                );
                (frames, starts, more_pty_ids, owned)
            })
            .await;
            let (frames, starts, more_pty_ids, owned) = match result {
                Ok(v) => v,
                Err(_) => {
                    if let Some(diagnostics) = diagnostics.as_mut() {
                        diagnostics.finish("sweep_error");
                    }
                    return;
                },
            };
            dirty_pty_ids.extend(more_pty_ids);
            tracked = owned;
            for start in starts {
                if let Some(diagnostics) = diagnostics.as_mut() {
                    diagnostics.attach_tab(start);
                }
            }
            for f in frames {
                if let Some(diagnostics) = diagnostics.as_mut() {
                    diagnostics.observe_value(&f);
                }
                yield Ok(data_frame(f));
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests;
