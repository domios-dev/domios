use super::*;
use crate::agent::provider::ReplyRelayTurn;

pub(super) fn pending() -> PendingReplyRelay {
    PendingReplyRelay {
        sender_tab: "dispatcher".into(),
        delivery_id: "delivery".into(),
        provider_id: "fixture".into(),
        session_id: "own-session".into(),
        binding_nonce: "nonce".into(),
        after_offset: 100,
        user_text: "opaque task".into(),
        turn_normalization: Default::default(),
        hops: 0,
        task: Some(WorkerTaskBinding {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            owner_epoch: 1,
        }),
    }
}
#[cfg(test)]
pub(super) fn source() -> RelaySource<'static> {
    RelaySource {
        provider_id: "fixture",
        session_id: "own-session",
        binding_nonce: "nonce",
    }
}
pub(super) fn completed() -> ReplyRelayEvidence {
    ReplyRelayEvidence::Answered(ReplyRelayTurn {
        user_turn_id: "user-100".into(),
        user_record_start: 100,
        user_text: "opaque task".into(),
        assistant_turn_id: "assistant-150".into(),
        answer: "<domios type=\"report\" status=\"in_review\">\nopaque result".into(),
        complete: true,
    })
}
