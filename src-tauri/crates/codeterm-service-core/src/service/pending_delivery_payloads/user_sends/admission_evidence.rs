use super::*;

#[derive(Serialize)]
pub(crate) struct UserAdmissionEvidence {
    #[serde(rename = "correlationId")]
    client_token: String,
    evidence: &'static str,
}

fn admission_evidence(present: bool, history_complete: bool, owner_failed: bool) -> &'static str {
    if present {
        "present"
    } else if history_complete && !owner_failed {
        "absent"
    } else {
        "unknown"
    }
}

impl UserSendStore {
    pub(crate) fn admission_evidence(
        &self,
        tab_id: &str,
        tokens: &[String],
        ledger_tokens: &[String],
    ) -> Result<Vec<UserAdmissionEvidence>, String> {
        if tokens.len() > 64 || tokens.iter().any(|token| token.len() > 4096) {
            return Err("admission evidence query exceeds metadata bound".into());
        }
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let index = &self.load(&tab, &mut data)?.index;
        let admissions = tab.admissions.lock();
        // An evicted receipt cannot prove absence of an earlier admission.
        let history_complete = index.next_sequence <= continuity::MAX_RECEIPTS as u64 + 1;
        let owner_failed = tab.failed.load(Ordering::Acquire);
        Ok(tokens
            .iter()
            .map(|token| {
                let present = ledger_tokens.contains(token)
                    || admissions.tokens.existing(token).is_some()
                    || index
                        .records
                        .iter()
                        .any(|record| record.client_token.as_ref() == Some(token))
                    || index
                        .receipts
                        .iter()
                        .any(|receipt| receipt.client_token.as_ref() == Some(token));
                UserAdmissionEvidence {
                    client_token: token.clone(),
                    evidence: admission_evidence(present, history_complete, owner_failed),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
