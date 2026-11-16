use super::*;
use std::collections::HashMap;

#[test]
fn a_current_attachment_reacquires_after_pty_generation_rebind() {
    let mut attachments = HashMap::new();
    let owner = Uuid::new_v4();
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 1, true),
        AttachmentDecision::Attach
    );
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 1, false),
        AttachmentDecision::Acknowledge
    );
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 2, false),
        AttachmentDecision::Attach
    );
    assert_eq!(attachments["pty"], (owner, 2));
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 2, false),
        AttachmentDecision::Acknowledge
    );
}

#[test]
fn a_foreign_or_detached_ack_cannot_reacquire_a_lease() {
    let mut attachments = HashMap::new();
    let owner = Uuid::new_v4();
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 1, false),
        AttachmentDecision::Reject
    );
    attachment_decision(&mut attachments, "pty", owner, 1, true);
    assert_eq!(
        attachment_decision(&mut attachments, "pty", Uuid::new_v4(), 2, false),
        AttachmentDecision::Reject
    );
    attachments.remove("pty");
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 2, false),
        AttachmentDecision::Reject
    );
}

#[test]
fn attachment_capacity_remains_bounded_while_rebinds_update_in_place() {
    let mut attachments: HashMap<_, _> = (0..MAX_PRIMARY_OUTPUT_ATTACHMENTS)
        .map(|id| (format!("pty-{id}"), (Uuid::new_v4(), 1)))
        .collect();
    let owner = attachments["pty-0"].0;
    assert_eq!(
        attachment_decision(&mut attachments, "overflow", Uuid::new_v4(), 1, true),
        AttachmentDecision::Reject
    );
    assert_eq!(
        attachment_decision(&mut attachments, "pty-0", owner, 2, false),
        AttachmentDecision::Attach
    );
    assert_eq!(attachments.len(), MAX_PRIMARY_OUTPUT_ATTACHMENTS);
}

#[test]
fn reasserted_attach_recovers_a_silent_refusal_without_resetting_an_existing_lease() {
    let mut attachments = HashMap::new();
    let owner = Uuid::new_v4();
    // The first transport-accepted update was ineligible and left no server entry.
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 1, true),
        AttachmentDecision::Attach
    );
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 1, true),
        AttachmentDecision::Acknowledge
    );
    assert_eq!(
        attachment_decision(&mut attachments, "pty", owner, 2, true),
        AttachmentDecision::Attach
    );
}
