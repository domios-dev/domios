use std::time::Instant;

use codeterm_types::pty::{TabStatus, TabStatusTracker, TimelineState};
use codeterm_util::lock_hold::{TimedMutex, TimedRwLock};

use super::{MultiplexerSnapshot, MultiplexerState};
use crate::multiplexer::types::Tab;

/// A tab's status from its PTY tracker. A live provider prompt outranks every
/// tracked state except a PTY confirmed dead, which cannot be waiting.
pub fn resolved_tab_status(
    tab: &Tab,
    tracker: &TabStatusTracker,
    resolve: impl Fn(&Tab, TabStatus) -> TabStatus,
) -> Option<TabStatus> {
    let tracked = tab
        .pty_id
        .as_deref()
        .and_then(|pty_id| tracker.get_status_if_tracked(pty_id));
    let raw = if tab.pending_prompt.is_some() && !matches!(tracked, Some(TabStatus::Dead)) {
        Some(TabStatus::Waiting)
    } else {
        tracked
    }?;
    Some(resolve(tab, raw))
}

/// The status every tab surface reports. A tab with no tracked PTY (a chat
/// backend, or a PTY not spawned yet) reports its provider-declared state.
pub fn effective_tab_status(tab: &Tab, resolved: Option<TabStatus>) -> TabStatus {
    resolved.unwrap_or_else(|| match tab.agent_state.as_ref().map(|s| s.state) {
        Some(TimelineState::Working) => TabStatus::Working,
        Some(TimelineState::Question) => TabStatus::Waiting,
        Some(TimelineState::Idle) | None => TabStatus::Idle,
    })
}

pub fn tab_status_label(tab: &Tab, resolved: Option<TabStatus>) -> &'static str {
    effective_tab_status(tab, resolved).label()
}

/// Waits for both snapshot locks in lock order until `deadline`. Concurrent
/// readers share `mux` and hold `agent_tracker` only to copy statuses, so a
/// reader never fails because another reader got there first.
pub fn snapshot_with_status_until(
    mux: &TimedRwLock<MultiplexerState>,
    tracker: &TimedMutex<TabStatusTracker>,
    deadline: Instant,
    resolve: impl Fn(&Tab, TabStatus) -> TabStatus,
) -> Option<MultiplexerSnapshot> {
    let mux = mux.try_read_until(deadline)?;
    let tracker = tracker.try_lock_until(deadline)?;
    Some(mux.snapshot_with_status_resolver(&tracker, resolve))
}
