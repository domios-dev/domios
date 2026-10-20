use super::*;
impl UserSendStore {
    pub(crate) fn runtime_envelope_id(
        &self,
        tab_id: &str,
        token: &str,
    ) -> Result<Option<String>, String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        let index = &self.load(&tab, &mut data)?.index;
        Ok(index
            .records
            .iter()
            .find(|record| record.client_token.as_deref() == Some(token))
            .map(|record| record.send_id.clone())
            .or_else(|| {
                index
                    .receipts
                    .iter()
                    .find(|receipt| receipt.client_token.as_deref() == Some(token))
                    .map(|receipt| receipt.send_id.clone())
            }))
    }
    pub(crate) fn admit_runtime_capture(
        &self,
        reservation: &UserReservation,
        text: &str,
        held: bool,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_question(
            reservation,
            text,
            text,
            None,
            None,
            held,
            RuntimeAdmission {
                capture_required: true,
                ..Default::default()
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authority() -> UserSendAuthority {
        UserSendAuthority {
            tab_id: "tab".into(),
            pty_id: None,
            binding_nonce: "nonce".into(),
            pty_session_generation: None,
            provider_id: "provider".into(),
            session_id: Some("thread".into()),
            observed_session_id: Some("thread".into()),
            prelaunch: false,
        }
    }
    #[test]
    fn runtime_envelope_cannot_overtake_an_earlier_preparation_when_its_own_slot_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(dir.path()).unwrap();
        let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
        let earlier = store
            .reserve(authority(), None, || ledger.reserve_identity())
            .unwrap();
        let later = store
            .reserve(authority(), None, || ledger.reserve_identity())
            .unwrap();
        let record = store
            .admit_runtime_envelope(&later, "opaque envelope", None)
            .unwrap();
        ledger.admit_user(&record);
        assert!(!store.eligible(&record));
        drop(later);
        assert!(!store.eligible(&record));
        assert!(!store.claim(&record, Some("thread")).unwrap());
        drop(earlier);
        assert!(store.eligible(&record));
        assert!(store.claim(&record, Some("thread")).unwrap());
    }
    #[test]
    fn a_restored_captured_input_keeps_its_frozen_capture_requirement_and_claim() {
        let dir = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(dir.path()).unwrap();
        let authority = authority();
        let reservation = store
            .reserve(authority, None, || (uuid::Uuid::new_v4().to_string(), 1))
            .unwrap();
        let record = store
            .admit_runtime_capture(&reservation, "opaque voice", false)
            .unwrap();
        assert!(record.capture_required);
        assert!(!store.eligible(&record));
        assert!(!store.claim(&record, Some("thread")).unwrap());
        drop(reservation);
        assert!(store.eligible(&record));
        assert!(store.claim(&record, Some("thread")).unwrap());
        let restored = UserSendStore::open(dir.path()).unwrap();
        let record = restored.records("tab").unwrap().remove(0);
        assert!(record.capture_required);
        assert_eq!(record.state, UserSendState::Claimed);
        assert!(!restored.eligible(&record));
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::*;
    #[test]
    fn envelope_idempotency_survives_restart_and_payload_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(dir.path()).unwrap();
        let authority = UserSendAuthority {
            tab_id: "tab".into(),
            pty_id: None,
            binding_nonce: "nonce".into(),
            pty_session_generation: None,
            provider_id: "provider".into(),
            session_id: Some("thread".into()),
            observed_session_id: Some("thread".into()),
            prelaunch: false,
        };
        let reservation = store
            .reserve(authority, Some("runtime-envelope:report".into()), || {
                (uuid::Uuid::new_v4().to_string(), 1)
            })
            .unwrap();
        let record = store.admit(&reservation, "opaque", "opaque", None).unwrap();
        drop(reservation);
        assert_eq!(
            store
                .runtime_envelope_id("tab", "runtime-envelope:report")
                .unwrap()
                .as_deref(),
            Some(record.send_id.as_str())
        );
        store.claim(&record, Some("thread")).unwrap();
        store.finish_handoff(&record, false).unwrap();
        store.complete(&record).unwrap();
        let reopened = UserSendStore::open(dir.path()).unwrap();
        assert_eq!(
            reopened
                .runtime_envelope_id("tab", "runtime-envelope:report")
                .unwrap()
                .as_deref(),
            Some(record.send_id.as_str())
        );
        assert!(reopened
            .runtime_envelope_id("other", "runtime-envelope:report")
            .unwrap()
            .is_none());
        assert!(reopened.records("tab").unwrap().is_empty());
    }
    #[test]
    fn late_live_proof_upgrades_the_retired_receipt_after_payload_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(dir.path()).unwrap();
        let ledger = crate::service::chat_send_transaction::ChatSendLedger::default();
        let authority = UserSendAuthority {
            tab_id: "tab".into(),
            pty_id: None,
            binding_nonce: "nonce".into(),
            pty_session_generation: None,
            provider_id: "provider".into(),
            session_id: Some("thread".into()),
            observed_session_id: Some("thread".into()),
            prelaunch: false,
        };
        let reservation = store
            .reserve(authority, Some("envelope".into()), || {
                ledger.reserve_identity()
            })
            .unwrap();
        let record = store.admit(&reservation, "opaque", "opaque", None).unwrap();
        ledger.admit_user(&record);
        drop(reservation);
        assert!(store.claim(&record, Some("thread")).unwrap());
        store.finish_handoff(&record, false).unwrap();
        assert!(store.complete(&record).unwrap());
        assert!(store.records("tab").unwrap().is_empty());
        let mut status = ledger.status(&record.send_id).unwrap();
        status.state = crate::service::chat_send_transaction::ChatSendState::Sent;
        store.persist_runtime_status(&status).unwrap();
        let reopened = UserSendStore::open(dir.path()).unwrap();
        assert_eq!(
            reopened.receipts("tab").unwrap()[0].state,
            continuity::ReceiptState::Delivered
        );
    }
}
