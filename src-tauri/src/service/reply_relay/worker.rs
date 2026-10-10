use super::model::{
    matches_source, matches_user_turn, PendingReplyRelay, RelaySource, WorkerTaskBinding,
};
use crate::agent::provider::ReplyRelayEvidence;
use crate::agent::reports::ReportStatus;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, PartialEq, Eq)]
pub struct WorkerTurnReport {
    pub status: ReportStatus,
    pub body: String,
    pub incident: bool,
}

pub fn validate_worker_lifecycle_capabilities(
    tool_less: bool,
    reply_relay: bool,
) -> Result<(), String> {
    if tool_less && !reply_relay {
        Err("A tool-less plan Worker requires authoritative reply-relay support for host ACK and reporting".into())
    } else {
        Ok(())
    }
}

pub(super) fn host_startup_ack_eligible(
    tool_less: bool,
    supported: bool,
    fresh_session: bool,
    exact_first_user: bool,
) -> bool {
    tool_less && supported && fresh_session && exact_first_user
}

pub fn worker_ack_evidence_eligible(
    tool_less: bool,
    pending: &PendingReplyRelay,
    source: RelaySource<'_>,
    exact_user_turn: bool,
) -> bool {
    tool_less && pending.task.is_some() && matches_source(pending, source) && exact_user_turn
}

#[derive(Default)]
pub struct WorkerTurnState {
    acknowledged: HashSet<WorkerTaskBinding>,
    reported_through: HashMap<WorkerTaskBinding, u64>,
    unavailable: bool,
    pending: VecDeque<PendingReplyRelay>,
}

impl WorkerTurnState {
    pub fn track(&mut self, pending: PendingReplyRelay) {
        if pending.task.is_some()
            && self.pending.len() < super::model::MAX_TRACKED_RELAY_DELIVERIES
            && !self
                .pending
                .iter()
                .any(|tracked| tracked.delivery_id == pending.delivery_id)
        {
            self.pending.push_back(pending);
        }
    }

    pub fn cancel(&mut self, delivery_id: &str) {
        self.pending
            .retain(|pending| pending.delivery_id != delivery_id);
    }

    pub fn pending(&self) -> Vec<PendingReplyRelay> {
        self.pending.iter().cloned().collect()
    }

    pub fn read_offset(&self, pending: &PendingReplyRelay) -> u64 {
        pending
            .task
            .as_ref()
            .and_then(|task| self.reported_through.get(task))
            .map_or(pending.after_offset, |through| {
                pending.after_offset.max(through.saturating_add(1))
            })
    }

    pub fn needs_ack(
        &self,
        pending: &PendingReplyRelay,
        source: RelaySource<'_>,
        exact_user_turn: bool,
    ) -> bool {
        worker_ack_evidence_eligible(true, pending, source, exact_user_turn)
            && pending
                .task
                .as_ref()
                .is_some_and(|task| !self.acknowledged.contains(task))
    }

    pub fn mark_acknowledged(&mut self, task: WorkerTaskBinding) {
        self.acknowledged.insert(task);
    }

    pub fn observe_completion(
        &mut self,
        pending: &PendingReplyRelay,
        source: RelaySource<'_>,
        evidence: Option<&ReplyRelayEvidence>,
    ) -> Option<WorkerTurnReport> {
        let (evidence, status) =
            super::report_marker::prepare_completed_reply(pending, source, evidence);
        self.observe_prepared_completion(pending, source, evidence.as_ref(), status)
    }

    pub(super) fn observe_prepared_completion(
        &mut self,
        pending: &PendingReplyRelay,
        source: RelaySource<'_>,
        evidence: Option<&ReplyRelayEvidence>,
        report_status: Option<ReportStatus>,
    ) -> Option<WorkerTurnReport> {
        let task = pending.task.as_ref()?;
        if !matches_source(pending, source) || self.unavailable {
            return None;
        }
        let evidence = evidence.filter(|evidence| evidence.is_valid())?;
        let (start, text, id, report) = match evidence {
            ReplyRelayEvidence::Answered(turn) => (Some(turn.user_record_start), Some(turn.user_text.as_str()), Some(turn.user_turn_id.as_str()), report_status.map(|status| WorkerTurnReport {status, body: turn.answer.clone(), incident: false})),
            ReplyRelayEvidence::Ended(turn) => (Some(turn.user_record_start), Some(turn.user_text.as_str()), Some(turn.user_turn_id.as_str()), Some(WorkerTurnReport {status: ReportStatus::Blocked, body: format!("Worker turn ended: {}", turn.outcome.as_str()), incident: false})),
            ReplyRelayEvidence::Unavailable(state) => (None, None, None, Some(WorkerTurnReport {status: ReportStatus::Blocked, body: format!("Coordination incident: reply relay is unavailable for this Worker session: {}", state.reason), incident: true})),
        };
        if let Some(start) = start {
            if !matches_user_turn(pending, id?, start, text?)
                || self
                    .reported_through
                    .get(task)
                    .is_some_and(|through| start <= *through)
            {
                return None;
            }
            self.reported_through.insert(task.clone(), start);
        } else {
            self.unavailable = true;
        }
        self.pending
            .retain(|tracked| tracked.delivery_id != pending.delivery_id);
        if self.unavailable {
            self.pending.clear();
        }
        report
    }
}

#[cfg(test)]
mod tests;

pub(super) fn completed_brief_ack_eligible(
    pending: &PendingReplyRelay,
    source: RelaySource<'_>,
    evidence: Option<&ReplyRelayEvidence>,
) -> bool {
    let Some(ReplyRelayEvidence::Answered(turn)) = evidence else {
        return false;
    };
    pending.task.is_some()
        && turn.complete
        && matches_source(pending, source)
        && matches_user_turn(
            pending,
            &turn.user_turn_id,
            turn.user_record_start,
            &turn.user_text,
        )
}
