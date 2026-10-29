use codeterm_types::pty::TabStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepPolicy {
    pub enabled: bool,
    pub idle_minutes: u64,
}

impl Default for SleepPolicy {
    fn default() -> Self {
        Self::from(&codeterm_types::config::tab_sleep::TabSleepConfig::default())
    }
}

impl From<&codeterm_types::config::tab_sleep::TabSleepConfig> for SleepPolicy {
    fn from(config: &codeterm_types::config::tab_sleep::TabSleepConfig) -> Self {
        Self {
            enabled: config.enabled,
            idle_minutes: config.idle_minutes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastTurnEvidence {
    pub session_id: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedSessionSource {
    Disk,
    RuntimeStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedSession {
    pub session_id: String,
    pub restorable_launch: bool,
    pub source: Option<PersistedSessionSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SleepFacts {
    pub selected_in_any_client: bool,
    pub general: bool,
    pub protected: bool,
    pub status: Option<TabStatus>,
    pub active_turn: bool,
    pub live_pty: bool,
    pub last_pty_output_ms: Option<u64>,
    pub last_turn: Option<LastTurnEvidence>,
    pub held_send: bool,
    pub written_send: bool,
    pub unconfirmed_send: bool,
    pub parked_delivery: bool,
    pub assigned_task: bool,
    /// Independent utility shells count only when they report to this tab.
    pub reporting_child: bool,
    /// Provider-owned background terminals exclude independent utility shells.
    pub owned_background_terminal: bool,
    pub bound_cron: bool,
    pub pending_report: bool,
    pub external_chat_subscriber: bool,
    pub mesh_relay: bool,
    pub last_activity_ms: Option<u64>,
    pub last_open_ms: Option<u64>,
    pub exact_resume_capability: bool,
    pub persisted_session: Option<PersistedSession>,
}

pub trait SleepFactsGatherer {
    /// The caller holds the mux lock; unknown safety facts return None.
    fn gather_sleep_facts(&self, tab_id: &str) -> Option<SleepFacts>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StayReason {
    Disabled,
    Selected,
    General,
    Protected,
    NotIdle,
    ActiveTurn,
    PtyActivity,
    LastTurnNotCompleted,
    ForeignTurnEvidence,
    HeldSend,
    WrittenSend,
    UnconfirmedSend,
    ParkedDelivery,
    AssignedTask,
    ReportingChild,
    OwnedBackgroundTerminal,
    BoundCron,
    PendingReport,
    ExternalChatSubscriber,
    MeshRelay,
    ActivityNotOldEnough,
    OpenNotOldEnough,
    ExactResumeUnsupported,
    MissingSession,
    MissingRestorableLaunch,
    MissingSessionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepDecision {
    Sleep,
    Stay(StayReason),
}

pub fn evaluate(policy: &SleepPolicy, facts: &SleepFacts, now_ms: u64) -> SleepDecision {
    use StayReason::*;

    let guards = [
        (!policy.enabled, Disabled),
        (facts.selected_in_any_client, Selected),
        (facts.general, General),
        (facts.protected, Protected),
        (facts.status != Some(TabStatus::Idle), NotIdle),
        (facts.active_turn, ActiveTurn),
        (facts.held_send, HeldSend),
        (facts.written_send, WrittenSend),
        (facts.unconfirmed_send, UnconfirmedSend),
        (facts.parked_delivery, ParkedDelivery),
        (facts.assigned_task, AssignedTask),
        (facts.reporting_child, ReportingChild),
        (facts.owned_background_terminal, OwnedBackgroundTerminal),
        (facts.bound_cron, BoundCron),
        (facts.pending_report, PendingReport),
        (facts.external_chat_subscriber, ExternalChatSubscriber),
        (facts.mesh_relay, MeshRelay),
        (!facts.exact_resume_capability, ExactResumeUnsupported),
    ];
    if let Some((_, reason)) = guards.into_iter().find(|(blocked, _)| *blocked) {
        return SleepDecision::Stay(reason);
    }
    let Some(session) = facts
        .persisted_session
        .as_ref()
        .filter(|session| !session.session_id.trim().is_empty())
    else {
        return SleepDecision::Stay(MissingSession);
    };
    if !session.restorable_launch {
        return SleepDecision::Stay(MissingRestorableLaunch);
    }
    if session.source.is_none() {
        return SleepDecision::Stay(MissingSessionSource);
    }
    let Some(turn) = facts.last_turn.as_ref().filter(|turn| turn.completed) else {
        return SleepDecision::Stay(LastTurnNotCompleted);
    };
    if turn.session_id != session.session_id {
        return SleepDecision::Stay(ForeignTurnEvidence);
    }
    let threshold_ms = policy.idle_minutes.saturating_mul(60_000);
    if facts.live_pty
        && !facts
            .last_pty_output_ms
            .and_then(|last| now_ms.checked_sub(last))
            .is_some_and(|idle| idle > threshold_ms)
    {
        return SleepDecision::Stay(PtyActivity);
    }
    let (last, reason) = match (facts.last_activity_ms, facts.last_open_ms) {
        (Some(activity), Some(open)) if open > activity => (open, OpenNotOldEnough),
        (Some(activity), _) => (activity, ActivityNotOldEnough),
        (None, Some(open)) => (open, OpenNotOldEnough),
        (None, None) => return SleepDecision::Stay(ActivityNotOldEnough),
    };
    if !now_ms
        .checked_sub(last)
        .is_some_and(|idle| idle > threshold_ms)
    {
        return SleepDecision::Stay(reason);
    }
    SleepDecision::Sleep
}

#[cfg(test)]
mod tests;
