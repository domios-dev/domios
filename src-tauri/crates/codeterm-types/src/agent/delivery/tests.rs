use super::*;

#[test]
fn status_keeps_null_error_and_omits_unset_metadata() {
    let status = ChatSendStatus {
        send_id: "send".into(),
        tab_id: "tab".into(),
        state: ChatSendState::Pending,
        error: None,
        ledger_id: "ledger".into(),
        sequence: 1,
        retained_from: 1,
        correlation_id: None,
        matched_row: None,
        pending_reason: None,
        system_send: None,
    };
    let result = ChatSendResult {
        status,
        pad_file: None,
    };
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({
            "send_id": "send", "tab_id": "tab", "state": "held", "error": null,
            "ledger_id": "ledger", "sequence": 1, "retained_from": 1
        })
    );
}
