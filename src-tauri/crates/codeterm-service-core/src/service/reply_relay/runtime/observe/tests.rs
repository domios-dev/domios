use super::*;
use crate::agent::provider::{ReplyRelayEvidence, ReplyRelayTurn};
use crate::agent::reports::ReportStatus;
use crate::service::messenger::Messenger;

fn pending() -> PendingReplyRelay {
    PendingReplyRelay {
        sender_tab: "source-tab".into(),
        delivery_id: "delivery-id".into(),
        provider_id: "provider-id".into(),
        session_id: "session-id".into(),
        binding_nonce: "binding-nonce".into(),
        after_offset: 10,
        user_text: "opaque-request".into(),
        turn_normalization: Default::default(),
        hops: 0,
        task: None,
    }
}

fn source() -> RelaySource<'static> {
    RelaySource {
        provider_id: "provider-id",
        session_id: "session-id",
        binding_nonce: "binding-nonce",
    }
}

#[test]
fn reply_relay_frames_acknowledgement_and_status_fragments_as_reports() {
    for fragment in ["opaque-fragment-a", "opaque-fragment-b"] {
        let routed = Messenger::from(reply_relay_envelope(
            "source-label".into(),
            "source-tab",
            None,
        ))
        .wrap(fragment);
        let parsed = codeterm_wire::domios::parse_message(&routed).expect("routed frame parses");

        assert_eq!(parsed.attr("from"), Some("report"));
        assert_eq!(parsed.attr("label"), Some("source-label"));
        assert_eq!(parsed.attr("tab"), Some("source-tab"));
        assert!(!routed.contains("<tip>"));
        assert!(!routed.contains(Messenger::AGENT_REPLY_HINT_PREFIX));
    }
}

#[test]
fn status_marker_without_a_body_does_not_create_a_routed_request() {
    let evidence = ReplyRelayEvidence::Answered(ReplyRelayTurn {
        user_turn_id: "user-turn".into(),
        user_record_start: 10,
        user_text: "opaque-request".into(),
        assistant_turn_id: "assistant-turn".into(),
        answer: "<domios type=\"report\" status=\"partial\">\n".into(),
        complete: true,
    });
    let (prepared, status) = super::super::super::report_marker::prepare_completed_reply(
        &pending(),
        source(),
        Some(&evidence),
    );

    assert_eq!(status, Some(ReportStatus::Partial));
    let actions = super::super::super::model::decide_reply_relay_evidence(
        &pending(),
        source(),
        prepared.as_ref(),
        true,
        8,
        false,
    );
    assert_eq!(actions, ReplyRelayDecision::Wait);
}
