//! Multiplexed web output stream (D1 §5): ONE SSE connection carrying every
//! tab's PTY output as tab-tagged frames, replacing the per-tab 25s
//! long-poll. Cursor = per-tab absolute byte offset over the EXISTING
//! `OutputBuffer` (the replay ring); `boot_id` is shared with the events stream
//! so a daemon restart invalidates both cursors consistently.

use crate::stream_census::{track, StreamGuard, StreamKind};
use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use base64::Engine;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use tokio::sync::broadcast;

use crate::routes::{check_auth, AppState};
use crate::stream_diagnostics::{configured_level, StreamDiagnostics, StreamTabStart};
use crate::tab_routes::resolve_pty_id_with_fallback;
use codeterm_service_core::agent::status::TabStatus;
use codeterm_service_core::service::output_stream_activity::OutputStreamActivity;
use codeterm_service_core::service::ServiceState;

/// Per-sweep per-tab byte cap: round-robin fairness so one noisy tab can't
/// starve the others. Leftover bytes are read on the next sweep.
const PER_TAB_SWEEP_CAP: usize = 128 * 1024;

#[derive(Deserialize)]
pub struct OutputStreamQuery {
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

#[derive(Default)]
struct ActivityWake {
    dirty_pty_ids: HashSet<String>,
    mux_changed: bool,
    read_all: bool,
}

fn record_activity(
    wake: &mut ActivityWake,
    activity: OutputStreamActivity,
    pty_filter: Option<&str>,
) {
    match activity {
        OutputStreamActivity::Pty(pty)
            if pty_filter.is_none_or(|wanted| wanted == pty.as_str()) =>
        {
            wake.dirty_pty_ids.insert(pty);
        }
        OutputStreamActivity::Pty(_) => {}
        OutputStreamActivity::Mux => wake.mux_changed = true,
    }
}

async fn wait_for_activity(
    activity: &mut broadcast::Receiver<OutputStreamActivity>,
    pty_filter: Option<&str>,
) -> Option<ActivityWake> {
    let mut wake = ActivityWake::default();
    match activity.recv().await {
        Ok(event) => record_activity(&mut wake, event, pty_filter),
        Err(broadcast::error::RecvError::Lagged(_)) => wake.read_all = true,
        Err(broadcast::error::RecvError::Closed) => return None,
    }
    loop {
        match activity.try_recv() {
            Ok(event) => record_activity(&mut wake, event, pty_filter),
            Err(broadcast::error::TryRecvError::Lagged(_)) => wake.read_all = true,
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Closed) => return None,
        }
    }
    Some(wake)
}

#[derive(Debug, PartialEq)]
pub(crate) struct TabOutputFrame {
    output: String,
    offset: u64,
}

fn drain_tab_output(
    output_buf: &codeterm_service_core::OutputBufferState,
    pty_id: &str,
    offset: &mut u64,
) -> Vec<TabOutputFrame> {
    let buffer = output_buf.lock();
    drain_buffered_tab_output(&buffer, pty_id, offset)
}

fn drain_buffered_tab_output(
    buffer: &codeterm_service_core::pty::manager::OutputBuffer,
    pty_id: &str,
    offset: &mut u64,
) -> Vec<TabOutputFrame> {
    let mut frames = Vec::new();
    loop {
        let chunk = buffer.read_since_limited(pty_id, *offset, PER_TAB_SWEEP_CAP);
        if chunk.bytes.is_empty() {
            break;
        }
        let read = plan_emit_limited(*offset, &chunk);
        if read.output.is_empty() {
            break;
        }
        *offset = read.new_offset;
        frames.push(TabOutputFrame {
            output: String::from_utf8_lossy(&read.output).into_owned(),
            offset: read.new_offset,
        });
    }
    frames
}

pub(crate) fn tab_output_frames(
    mut activity: broadcast::Receiver<OutputStreamActivity>,
    output_buf: codeterm_service_core::OutputBufferState,
    pty_id: Option<String>,
    initial_offset: u64,
) -> impl Stream<Item = TabOutputFrame> + Send + 'static {
    async_stream::stream! {
        let Some(pty_id) = pty_id else {
            futures_util::future::pending::<()>().await;
            return;
        };
        let mut offset = initial_offset;
        loop {
            let immediate = output_buf.try_lock().map(|buffer| {
                drain_buffered_tab_output(&buffer, &pty_id, &mut offset)
            });
            let frames = if let Some(frames) = immediate {
                frames
            } else {
                let buffer = std::sync::Arc::downgrade(&output_buf);
                let target = pty_id.clone();
                let Ok(Some((frames, next_offset))) = tokio::task::spawn_blocking(move || {
                    let buffer = buffer.upgrade()?;
                    let mut offset = offset;
                    let frames = drain_tab_output(&buffer, &target, &mut offset);
                    Some((frames, offset))
                }).await else { return; };
                offset = next_offset;
                frames
            };
            for frame in frames {
                yield frame;
            }
            let Some(wake) = wait_for_activity(&mut activity, Some(&pty_id)).await else {
                return;
            };
            if wake.read_all || wake.dirty_pty_ids.contains(&pty_id) {
                continue;
            }
        }
    }
}

fn tab_output_start_offset(from: Option<u64>, total: u64) -> u64 {
    from.map_or(total, |offset| offset.min(total))
}

#[derive(Deserialize)]
pub struct StreamQuery {
    from: Option<u64>,
}

pub async fn stream_tab_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<
    Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;
    let activity = state
        .svc
        .ext::<codeterm_service_core::service::output_stream_activity::OutputStreamActivityTx>()
        .0
        .subscribe();
    let owner = state.clone();
    let (pty_id, output_buf, initial_total) = tokio::task::spawn_blocking(move || {
        let state = owner;
        let chat = state.svc.tab_chat_backing(&raw_tab_id).is_some();
        let pty_id = if chat {
            None
        } else {
            Some(resolve_pty_id_with_fallback(&state, &raw_tab_id)?)
        };
        let output_buf = state.svc.output_buf.clone();
        let initial_total = pty_id.as_ref().map_or(0, |pty_id| {
            let output = output_buf.lock();
            output.total_written(pty_id)
        });
        Ok::<_, (StatusCode, &'static str)>((pty_id, output_buf, initial_total))
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed"))??;
    let offset = tab_output_start_offset(query.from, initial_total);
    let frames = tab_output_frames(activity, output_buf, pty_id, offset);
    let stream = frames.map(|frame| {
        Ok(Event::default()
            .data(serde_json::json!({"output": frame.output, "offset": frame.offset}).to_string()))
    });
    Ok(
        Sse::new(track(stream, StreamGuard::local(StreamKind::TabOutput)))
            .keep_alive(KeepAlive::default()),
    )
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
fn plan_emit_limited(offset: u64, chunk: &codeterm_pty::pty::manager::OutputRead) -> TabRead {
    let end = chunk.start + chunk.bytes.len() as u64;
    let mut read = plan_emit_with_boundary(
        offset,
        &chunk.bytes,
        end,
        PER_TAB_SWEEP_CAP,
        chunk.following_continuation,
    );
    read.has_more |= end < chunk.total;
    log::debug!(
        "[OUTPUT-COPY] copied_bytes={} offset={} total={}",
        chunk.bytes.len(),
        chunk.start,
        chunk.total
    );
    read
}

#[cfg(test)]
fn plan_emit(offset: u64, chunk: &[u8], total: u64, cap: usize) -> TabRead {
    plan_emit_with_boundary(offset, chunk, total, cap, false)
}

fn plan_emit_with_boundary(
    offset: u64,
    chunk: &[u8],
    total: u64,
    cap: usize,
    following_continuation: bool,
) -> TabRead {
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
    let mut take = utf8_complete_prefix(chunk, cap, following_continuation);
    if take == 0 {
        if chunk.len() < 4 && cap >= chunk.len() {
            // Only an incomplete character so far: wait for its remaining bytes.
            return TabRead {
                truncated_from,
                output: Vec::new(),
                new_offset: base,
                has_more: false,
            };
        }
        take = chunk.len().min(cap); // invalid bytes: guarantee forward progress
    }
    let new_offset = base + take as u64;
    TabRead {
        truncated_from,
        output: chunk[..take].to_vec(),
        new_offset,
        has_more: cap < chunk.len(),
    }
}

fn utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

fn utf8_sequence_len(lead: u8) -> usize {
    match lead {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// Length of `chunk[..limit]` that ends on a UTF-8 character boundary, so a
/// character split by the cap or by a partial PTY read is never decoded in halves.
fn utf8_complete_prefix(chunk: &[u8], limit: usize, following_continuation: bool) -> usize {
    let mut take = limit.min(chunk.len());
    while take > 0
        && if take < chunk.len() {
            utf8_continuation(chunk[take])
        } else {
            following_continuation
        }
    {
        take -= 1;
    }
    if let Some(lead) = (take.saturating_sub(4)..take)
        .rev()
        .find(|&i| !utf8_continuation(chunk[i]))
    {
        if lead + utf8_sequence_len(chunk[lead]) > take {
            take = lead;
        }
    }
    take
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
            let retained_from = total_written.saturating_sub(buf.full_len(pty) as u64);
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

pub(crate) fn output_ended_frame(tab: &str, ended: &mut bool) -> Option<serde_json::Value> {
    if *ended {
        return None;
    }
    *ended = true;
    Some(serde_json::json!({ "tab_id": tab, "ended": true }))
}

pub(crate) fn output_bytes_frame(
    tab: &str,
    pty: &str,
    boot: &str,
    data: &[u8],
    offset: u64,
) -> serde_json::Value {
    serde_json::json!({
        "tab_id": tab, "pty_id": pty, "boot": boot,
        "output": String::from_utf8_lossy(data), "data": data, "offset": offset
    })
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
            let retained_from = total_written.saturating_sub(buf.full_len(pty) as u64);
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
            let chunk = buf.read_since_limited(pty, t.offset, PER_TAB_SWEEP_CAP);
            let read = plan_emit_limited(t.offset, &chunk);
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
            frames.push(output_bytes_frame(
                tab,
                pty,
                &boot,
                &read.output,
                read.new_offset,
            ));
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
                if let Some(frame) = output_ended_frame(&tab, &mut t.ended) {
                    frames.push(frame);
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

    let mut guard = StreamGuard::local(StreamKind::Output);
    guard.diagnostics = build_stream_diagnostics(
        diagnostic_level,
        session_id,
        scope,
        cursor_boot,
        cursor_offset,
    );
    let stream = async_stream::stream! {
        let mut guard = guard;
        let mut activity = svc.ext::<codeterm_service_core::service::output_stream_activity::OutputStreamActivityTx>().0.subscribe();
        let mut dirty_pty_ids = HashSet::new();
        let mut read_all = true;
        let mut sweep_now = true;
        let svc0 = svc.clone();
        let (boot_frame, mut tracked, starts) =
            match tokio::task::spawn_blocking({
                let tab_id = tab_id.clone();
                move || seed_initial(&svc0, cursor, tab_id.as_deref())
            }).await {
                Ok(v) => v,
                Err(_) => {
                    if let Some(diagnostics) = guard.diagnostics.as_mut() {
                        diagnostics.finish("seed_error");
                    }
                    return;
                },
            };
        for start in starts {
            if let Some(diagnostics) = guard.diagnostics.as_mut() {
                diagnostics.attach_tab(start);
            }
        }
        if let Some(bf) = boot_frame {
            if let Some(diagnostics) = guard.diagnostics.as_mut() {
                diagnostics.observe_value(&bf);
            }
            yield Ok(data_frame(bf));
        }
        loop {
            if !read_all && !sweep_now && dirty_pty_ids.is_empty() {
                let Some(wake) = wait_for_activity(&mut activity, None).await else {
                    if let Some(diagnostics) = guard.diagnostics.as_mut() {
                        diagnostics.finish("activity_closed");
                    }
                    return;
                };
                dirty_pty_ids.extend(wake.dirty_pty_ids);
                read_all |= wake.read_all;
            }
            let dirty_for_sweep = std::mem::take(&mut dirty_pty_ids);
            let read_all_for_sweep = std::mem::replace(&mut read_all, false);
            sweep_now = false;
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
                    if let Some(diagnostics) = guard.diagnostics.as_mut() {
                        diagnostics.finish("sweep_error");
                    }
                    return;
                },
            };
            dirty_pty_ids.extend(more_pty_ids);
            tracked = owned;
            for start in starts {
                if let Some(diagnostics) = guard.diagnostics.as_mut() {
                    diagnostics.attach_tab(start);
                }
            }
            for f in frames {
                if let Some(diagnostics) = guard.diagnostics.as_mut() {
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
