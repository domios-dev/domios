use super::*;
impl UserSendStore {
    pub(crate) fn admit_runtime_capture(
        &self,
        reservation: &UserReservation,
        text: &str,
        held: bool,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_question(reservation, text, text, None, None, held, None, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_restored_captured_input_keeps_its_frozen_capture_requirement_and_claim() {
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
            .reserve(authority, None, || (uuid::Uuid::new_v4().to_string(), 1))
            .unwrap();
        let record = store
            .admit_runtime_capture(&reservation, "opaque voice", false)
            .unwrap();
        assert!(record.capture_required);
        assert!(store.claim(&record, Some("thread")).unwrap());
        drop(reservation);
        let restored = UserSendStore::open(dir.path()).unwrap();
        let record = restored.records("tab").unwrap().remove(0);
        assert!(record.capture_required);
        assert_eq!(record.state, UserSendState::Claimed);
        assert!(!restored.eligible(&record));
    }
}
