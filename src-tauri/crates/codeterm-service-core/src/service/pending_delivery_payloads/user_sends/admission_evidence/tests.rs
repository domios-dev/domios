use super::*;

#[test]
fn non_durable_backend_reports_own_ledger_presence_without_fabricating_absence() {
    let evidence = UserSendStore::non_durable_admission_evidence(
        &["known".into(), "missing".into()],
        &["known".into()],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(evidence).unwrap(),
        serde_json::json!([
            {"correlationId": "known", "evidence": "present"},
            {"correlationId": "missing", "evidence": "not_applicable"},
        ])
    );
    assert!(UserSendStore::non_durable_admission_evidence(&vec!["x".into(); 65], &[]).is_err());
}

#[test]
fn absence_requires_complete_healthy_history_and_no_admission() {
    for present in [false, true] {
        for complete in [false, true] {
            for failed in [false, true] {
                assert_eq!(
                    admission_evidence(present, complete, failed),
                    if present {
                        "present"
                    } else if complete && !failed {
                        "absent"
                    } else {
                        "unknown"
                    }
                );
            }
        }
    }
}

#[test]
fn receipts_ledger_and_in_flight_admissions_are_never_reported_absent() {
    let temp = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(temp.path()).unwrap();
    let tab = store.tab("tab");
    {
        let mut data = tab.data.lock();
        store
            .load(&tab, &mut data)
            .unwrap()
            .index
            .receipts
            .push(continuity::UserSendReceipt {
                send_id: "settled".into(),
                tab_id: "tab".into(),
                client_token: Some("durable".into()),
                ledger_sequence: 1,
                state: continuity::ReceiptState::Delivered,
            });
    }
    tab.admissions
        .lock()
        .tokens
        .insert("pending".into(), "pending-send".into(), 2)
        .unwrap();
    let tokens = vec![
        "durable".into(),
        "ledger".into(),
        "pending".into(),
        "missing".into(),
    ];
    let evidence = store
        .admission_evidence("tab", &tokens, &["ledger".into()])
        .unwrap();
    assert_eq!(
        evidence
            .iter()
            .map(|item| item.evidence)
            .collect::<Vec<_>>(),
        ["present", "present", "present", "absent"]
    );
    tab.data.lock().as_mut().unwrap().index.next_sequence = 258;
    assert_eq!(
        store.admission_evidence("tab", &tokens, &[]).unwrap()[3].evidence,
        "unknown"
    );
}
