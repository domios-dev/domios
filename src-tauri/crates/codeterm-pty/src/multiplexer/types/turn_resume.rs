use serde::{Deserialize, Serialize};

/// Durable evidence that a tab's agent was mid-turn during one daemon start.
/// A later start owes the tab one continue until the mark is settled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnResumeMark {
    pub session_id: String,
    pub start_epoch: String,
    /// Explicit continuation requested for this tab; replaces the default text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<TurnResumeSender>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled: Option<TurnResumeSettlement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnResumeSender {
    pub tab_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_agent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnResumeSettlement {
    pub start_epoch: String,
    pub outcome: TurnResumeOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnResumeOutcome {
    /// Claimed for this start and handed to the post-ready send path once.
    Sent,
    Skipped(String),
}

impl TurnResumeMark {
    fn new(session_id: &str, start_epoch: &str, prompt: Option<String>) -> Self {
        Self {
            session_id: session_id.to_string(),
            start_epoch: start_epoch.to_string(),
            prompt,
            sender: None,
            settled: None,
        }
    }

    /// Recorded by an earlier start and not yet settled.
    pub fn owed_in(&self, start_epoch: &str) -> bool {
        self.settled.is_none() && self.start_epoch != start_epoch
    }

    fn pending_request_in(&self, start_epoch: &str) -> bool {
        self.settled.is_none() && self.start_epoch == start_epoch && self.prompt.is_some()
    }

    fn suppressed_in(&self, start_epoch: &str) -> bool {
        self.settled.as_ref().is_some_and(|settlement| {
            settlement.start_epoch == start_epoch
                && settlement.outcome == TurnResumeOutcome::Skipped("not_requested".into())
        })
    }
}

/// Fold one live turn observation into the mark: `working_session` is the
/// tab's own session while its agent is mid-turn, `None` once the turn ends.
/// An owed mark stays frozen until settled; a pending request survives idle.
pub fn observe_turn(
    mark: &mut Option<TurnResumeMark>,
    working_session: Option<&str>,
    start_epoch: &str,
) -> bool {
    let next = match (mark.as_ref(), working_session) {
        (Some(current), _) if current.suppressed_in(start_epoch) => return false,
        (Some(current), _) if current.owed_in(start_epoch) => return false,
        (Some(current), None) if current.pending_request_in(start_epoch) => return false,
        (Some(current), Some(session)) if current.pending_request_in(start_epoch) => {
            Some(TurnResumeMark {
                session_id: session.to_string(),
                ..current.clone()
            })
        }
        (_, Some(session)) => Some(TurnResumeMark::new(session, start_epoch, None)),
        (_, None) => None,
    };
    if *mark == next {
        return false;
    }
    *mark = next;
    true
}

/// Ask the next start to continue this tab with `prompt`, whatever its turn state.
pub fn request_continue(
    mark: &mut Option<TurnResumeMark>,
    session_id: &str,
    start_epoch: &str,
    prompt: String,
) {
    *mark = Some(TurnResumeMark::new(session_id, start_epoch, Some(prompt)));
}

pub fn request_continue_from(
    mark: &mut Option<TurnResumeMark>,
    session_id: &str,
    start_epoch: &str,
    prompt: String,
    sender: TurnResumeSender,
) {
    let mut requested = TurnResumeMark::new(session_id, start_epoch, Some(prompt));
    requested.sender = Some(sender);
    *mark = Some(requested);
}

/// Queue an update prompt for this restored start while preserving a working
/// mark's original epoch when one already exists.
pub fn request_continue_for_restored_start(
    mark: &mut Option<TurnResumeMark>,
    session_id: &str,
    start_epoch: &str,
    prompt: String,
    sender: Option<TurnResumeSender>,
) {
    if let Some(current) = mark.as_mut().filter(|mark| mark.owed_in(start_epoch)) {
        current.prompt = Some(prompt);
        current.sender = sender;
        return;
    }
    let staged_epoch = format!("post-update:{start_epoch}");
    let mut requested = TurnResumeMark::new(session_id, &staged_epoch, Some(prompt));
    requested.sender = sender;
    *mark = Some(requested);
}

/// Suppress both a saved owed mark and the caller's current working mark.
pub fn suppress_continue(
    mark: &mut Option<TurnResumeMark>,
    session_id: &str,
    start_epoch: &str,
) -> bool {
    if mark
        .as_ref()
        .is_some_and(|current| current.suppressed_in(start_epoch))
    {
        return false;
    }
    let current = mark.get_or_insert_with(|| TurnResumeMark::new(session_id, start_epoch, None));
    current.settled = Some(TurnResumeSettlement {
        start_epoch: start_epoch.to_string(),
        outcome: TurnResumeOutcome::Skipped("not_requested".into()),
    });
    true
}

/// Settle the mark owed in `start_epoch`. Only the first settlement applies.
pub fn settle_turn_resume(
    mark: &mut Option<TurnResumeMark>,
    start_epoch: &str,
    outcome: TurnResumeOutcome,
) -> bool {
    match mark {
        Some(current) if current.owed_in(start_epoch) => {
            current.settled = Some(TurnResumeSettlement {
                start_epoch: start_epoch.to_string(),
                outcome,
            });
            true
        }
        _ => false,
    }
}

/// Replace this start's `Sent` claim when the claim itself could not be made durable.
pub fn withdraw_sent_claim(mark: &mut Option<TurnResumeMark>, start_epoch: &str, reason: String) {
    let Some(settlement) = mark.as_mut().and_then(|current| current.settled.as_mut()) else {
        return;
    };
    if settlement.start_epoch == start_epoch && settlement.outcome == TurnResumeOutcome::Sent {
        settlement.outcome = TurnResumeOutcome::Skipped(reason);
    }
}

#[cfg(test)]
mod tests;
