use super::{
    retired_delivery_flags, retired_delivery_status, worker_brief_flagged,
    AgentDeliveryReceiptStatus,
};

#[test]
fn exact_turn_proof_retires_as_delivered() {
    assert_eq!(
        retired_delivery_status(Some(true)),
        AgentDeliveryReceiptStatus::Delivered
    );
}

#[test]
fn absent_turn_retires_as_submit_unconfirmed() {
    assert_eq!(
        retired_delivery_status(Some(false)),
        AgentDeliveryReceiptStatus::SubmitUnconfirmed
    );
}

#[test]
fn unknown_session_retires_as_submit_unconfirmed() {
    assert_eq!(
        retired_delivery_status(None),
        AgentDeliveryReceiptStatus::SubmitUnconfirmed
    );
}

#[test]
fn worker_brief_and_report_receipts_are_flagged() {
    assert!(worker_brief_flagged(Some(
        crate::multiplexer::types::AgentRole::Worker
    )));
    assert_eq!(retired_delivery_flags(true, true), (true, true));
    assert_eq!(retired_delivery_flags(false, true), (false, true));
}
