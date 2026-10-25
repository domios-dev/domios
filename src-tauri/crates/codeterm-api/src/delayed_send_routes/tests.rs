use super::{CreateDelayedMessageBody, UpdateDelayedMessageBody};
use axum::http::StatusCode;
use axum::Json;
use codeterm_service_core::delayed_send::{
    recurrence::RecurrenceRule, DelayedMessage, DelayedMessageState, DelayedMessageView,
};

fn rule() -> RecurrenceRule {
    RecurrenceRule {
        every_ms: 600_000,
        until_ms: Some(1_200_000),
        count: None,
    }
}

#[test]
fn delayed_create_edit_and_list_contracts_carry_recurrence() {
    let create: CreateDelayedMessageBody = serde_json::from_value(serde_json::json!({
        "text": "tick",
        "scheduledAtMs": 600_000,
        "createdByTab": "creator",
        "recurrence": { "everyMs": 600_000, "untilMs": 1_200_000 }
    }))
    .unwrap();
    assert_eq!(create.recurrence, Some(rule()));

    let edit: UpdateDelayedMessageBody = serde_json::from_value(serde_json::json!({
        "recurrence": { "everyMs": 600_000, "count": 3 }
    }))
    .unwrap();
    let edited = edit.recurrence.unwrap();
    assert_eq!(edited.every_ms, 600_000);
    assert_eq!(edited.count, Some(3));

    let listed = DelayedMessageView {
        message: DelayedMessage {
            turn_retry: None,
            id: "dmsg-1".into(),
            tab_id: "tab-1".into(),
            workspace_id: None,
            text: "tick".into(),
            scheduled_at_ms: 1_200_000,
            created_at_ms: 0,
            state: DelayedMessageState::Queued,
            error: None,
            session_id_at_create: None,
            created_by_tab: Some("creator".into()),
            created_by_label: None,
            created_by_model: None,
            recurrence: Some(rule()),
            series_start_at_ms: Some(600_000),
            tick_index: 1,
            skipped_ticks: 2,
            team_id: Some("team-1".into()),
            last_tick_unconfirmed: false,
            last_tick_send_sequence: 0,
            origin: codeterm_service_core::delayed_send::MessageOrigin::User,
            reason: None,
            ticks: Vec::new(),
        },
        current_session_id: None,
        session_changed: false,
    };
    let value = serde_json::to_value(listed).unwrap();
    assert_eq!(value["recurrence"]["everyMs"], 600_000);
    assert_eq!(value["scheduledAtMs"], 1_200_000);
    assert_eq!(value["skippedTicks"], 2);
    assert!(value.get("origin").is_none());
}

#[test]
fn patch_and_delete_of_a_system_entry_return_a_typed_refusal() {
    for id in ["system:heartbeat:pl:tab", "system:heartbeat:worker:tab"] {
        let (status, Json(body)) = super::mutation_refusal_response(id).unwrap();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.code, "system_entry_immutable");
        assert_eq!(body.origin, "system");
        let value = serde_json::to_value(&body).unwrap();
        assert_eq!(value["code"], "system_entry_immutable");
        assert_eq!(value["origin"], "system");
    }
    assert!(super::mutation_refusal_response("dmsg-1").is_none());
}
