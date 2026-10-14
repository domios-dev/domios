use super::*;

fn receipt(id: &str, sequence: u64) -> UserSendReceipt {
    UserSendReceipt {
        send_id: id.into(),
        tab_id: "tab".into(),
        client_token: Some(format!("token-{id}")),
        ledger_sequence: sequence,
        state: ReceiptState::Failed,
    }
}

#[test]
fn receipt_listing_restores_both_failures_in_order_with_tokens_after_a_new_session_send() {
    let ledger = ChatSendLedger::default();
    let first = receipt("first", 1);
    let second = receipt("second", 2);
    for item in [&second, &first, &second, &first] {
        ledger.restore_user_receipt(item);
    }
    let new_send = ledger
        .try_start("tab", "opaque-new-session-turn", None)
        .unwrap();
    let statuses = ledger.reconcile("tab");
    assert_eq!(
        statuses
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", new_send.send_id.as_str()]
    );
    for (status, receipt) in statuses.iter().zip([first, second]) {
        assert_eq!(status.state, ChatSendState::Failed);
        assert_eq!(status.correlation_id, receipt.client_token);
        assert_eq!(status.error.as_deref(), Some("launch_ended_before_write"));
    }
}

#[test]
fn durable_failure_can_end_only_an_inactive_held_status() {
    for current in [
        ChatSendState::Pending,
        ChatSendState::Written,
        ChatSendState::Unknown,
        ChatSendState::Sent,
        ChatSendState::Failed,
    ] {
        for active in [false, true] {
            for receipt in [ReceiptState::Failed, ReceiptState::Degraded] {
                assert_eq!(
                    receipt_ends_held_status(&current, active, receipt),
                    current == ChatSendState::Pending && !active
                );
            }
            assert!(!receipt_ends_held_status(
                &current,
                active,
                ReceiptState::Uncertain
            ));
            assert!(!receipt_ends_held_status(
                &current,
                active,
                ReceiptState::Delivered
            ));
        }
    }
}

#[test]
fn receipt_reconciliation_repairs_a_missed_settlement_on_an_existing_held_ledger() {
    let ledger = ChatSendLedger::default();
    let first = ledger.try_start("tab", "opaque-first", None).unwrap();
    let second = ledger.try_start("tab", "opaque-second", None).unwrap();
    for status in [&first, &second] {
        ledger.attach_client_token(&status.send_id, format!("token-{}", status.send_id));
        ledger.restore_user_receipt(&receipt(&status.send_id, status.sequence));
    }
    let statuses = ledger.reconcile("tab");
    assert_eq!(statuses.len(), 2);
    assert!(statuses
        .iter()
        .all(|status| status.state == ChatSendState::Failed));
    assert!(statuses
        .iter()
        .all(|status| status.correlation_id.is_some()));
}
