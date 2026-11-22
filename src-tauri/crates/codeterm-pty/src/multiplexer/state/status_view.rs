use std::time::Instant;

use codeterm_types::pty::{
    AgentLiveState, BootStage, PromptKind, QuestionSubtype, TabStatus, TabStatusTracker,
    TabVerdict, TimelineState,
};
use codeterm_util::lock_hold::{TimedMutex, TimedRwLock};

use super::{MultiplexerSnapshot, MultiplexerState};
use crate::multiplexer::types::Tab;

/// Chat transports publish their turn state; PTYs use the reconciled tracker.
/// A live prompt outranks tracked states except a confirmed dead or failed PTY.
pub fn resolved_tab_status(
    tab: &Tab,
    tracker: &TabStatusTracker,
    resolve: impl Fn(&Tab, TabStatus) -> TabStatus,
) -> Option<TabStatus> {
    if launch_failed(tab) {
        return Some(TabStatus::Error);
    }
    if tab.provider_signals.access_lost() {
        log::debug!("[provider-access] status waiting tab={}", tab.id);
        return Some(TabStatus::Waiting);
    }
    if tab.is_chat_backend() {
        return Some(effective_tab_status(tab, None));
    }
    let tracked = tab
        .pty_id
        .as_deref()
        .and_then(|pty_id| tracker.get_status_if_tracked(pty_id));
    let raw = if tab.pending_prompt.is_some()
        && !matches!(tracked, Some(TabStatus::Dead | TabStatus::Error))
    {
        Some(TabStatus::Waiting)
    } else {
        tracked
    }?;
    Some(resolve(tab, raw))
}

/// The status every tab surface reports. A tab with no tracked PTY (a chat
/// backend, or a PTY not spawned yet) reports its provider-declared state.
pub fn effective_tab_status(tab: &Tab, resolved: Option<TabStatus>) -> TabStatus {
    if launch_failed(tab) {
        return TabStatus::Error;
    }
    if tab.provider_signals.access_lost() {
        return TabStatus::Waiting;
    }
    let chat = tab.is_chat_backend();
    if chat && tab.pending_prompt.is_some() {
        return TabStatus::Waiting;
    }
    let live = tab
        .agent_state
        .as_ref()
        .filter(|state| !chat || state.subtype != Some(QuestionSubtype::Permission));
    resolved.unwrap_or_else(|| match live.map(|s| s.state) {
        Some(TimelineState::Working) => TabStatus::Working,
        Some(TimelineState::Question) => TabStatus::Waiting,
        Some(TimelineState::Idle) | None => TabStatus::Idle,
    })
}

pub(super) fn chat_backend_agent_state(tab: &Tab, status: TabStatus) -> AgentLiveState {
    let prompt_kind = tab
        .pending_prompt
        .as_ref()
        .filter(|prompt| prompt.request_id.is_some())
        .and(tab.pending_prompt_kind);
    let current = tab
        .agent_state
        .as_ref()
        .filter(|state| state.subtype != Some(QuestionSubtype::Permission));
    if status == TabStatus::Waiting && prompt_kind == Some(PromptKind::Permission) {
        return TabVerdict {
            state: TimelineState::Question,
            subtype: Some(QuestionSubtype::Permission),
        }
        .into();
    }
    agent_state_for_status(current, status)
}

pub(super) fn agent_state_for_status(
    current: Option<&AgentLiveState>,
    status: TabStatus,
) -> AgentLiveState {
    let state = match status {
        TabStatus::Working => TimelineState::Working,
        TabStatus::Waiting => TimelineState::Question,
        TabStatus::Idle | TabStatus::Error | TabStatus::Dead => TimelineState::Idle,
    };
    current
        .filter(|current| current.state == state)
        .cloned()
        .unwrap_or_else(|| {
            TabVerdict {
                state,
                subtype: None,
            }
            .into()
        })
}

pub fn tab_status_label(tab: &Tab, resolved: Option<TabStatus>) -> &'static str {
    if launch_failed(tab) {
        return "error";
    }
    if tab.pending_prompt.is_some() || tab.provider_signals.access_lost() {
        return effective_tab_status(tab, resolved).label();
    }
    if tab.boot_stage.is_some_and(|stage| stage.is_provisional()) {
        return "unknown";
    }
    if tab.agent_booting {
        return "starting";
    }
    let agent = tab.agent_type.is_some() || tab.role.is_some() || tab.is_chat_backend();
    if agent {
        match tab.boot_stage {
            Some(BootStage::AgentReady | BootStage::PtyReattached) => {}
            Some(BootStage::ShellReady) if tab.agent_type.is_none() => return "unknown",
            Some(_) => return "starting",
            None if tab.agent_state.is_none() => return "unknown",
            None => {}
        }
        if tab.agent_type.is_none() && !tab.is_chat_backend() {
            return "unknown";
        }
        if tab.agent_type.is_some() && !tab.launch_session_authenticated() {
            return "unknown";
        }
    }
    if resolved.is_none() && tab.agent_state.is_none() {
        return "unknown";
    }
    effective_tab_status(tab, resolved).label()
}

fn launch_failed(tab: &Tab) -> bool {
    tab.boot_stage == Some(BootStage::SpawnFailed)
        || tab.agent_runtime_state.as_ref().is_some_and(|runtime| {
            matches!(
                runtime["state"].as_str(),
                Some("degraded" | "refused" | "unavailable")
            ) || runtime
                .get("failureCode")
                .is_some_and(|code| !code.is_null())
        })
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

#[cfg(test)]
mod tests;
