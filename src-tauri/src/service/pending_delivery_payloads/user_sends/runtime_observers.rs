use super::*;
use crate::service::{
    chat_send_transaction::{ChatSendState, ChatSendStatus},
    messenger::DeliveryWriteObserver,
};

#[derive(Default)]
pub(super) struct RuntimeAdmission {
    pub(super) question: Option<runtime_questions::QuestionResolution>,
    pub(super) capture_required: bool,
    pub(super) observer: Option<DeliveryWriteObserver>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct WriteObservers(HashMap<String, Observation>);
#[derive(Clone, Serialize, Deserialize)]
struct Observation {
    observer: DeliveryWriteObserver,
    consumed: bool,
}
impl WriteObservers {
    pub(super) fn insert(
        &mut self,
        records: &[UserSendRecord],
        receipts: &[continuity::UserSendReceipt],
        id: &str,
        observer: DeliveryWriteObserver,
    ) -> Result<(), String> {
        self.0.retain(|id, _| {
            records.iter().any(|record| record.send_id == *id)
                || receipts.iter().any(|receipt| receipt.send_id == *id)
        });
        if self.0.contains_key(id) || self.0.len() >= MAX_UNSETTLED + continuity::MAX_RECEIPTS {
            return Err("Runtime write observer admission capacity or identity refused".into());
        }
        self.0.insert(
            id.into(),
            Observation {
                observer,
                consumed: false,
            },
        );
        Ok(())
    }
    fn consume(&mut self, id: &str, state: ChatSendState) -> Option<DeliveryWriteObserver> {
        if !matches!(state, ChatSendState::Written | ChatSendState::Sent) {
            return None;
        }
        let entry = self.0.get_mut(id)?;
        if entry.consumed {
            return None;
        }
        entry.consumed = true;
        Some(entry.observer.clone())
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.0.len() > MAX_UNSETTLED + continuity::MAX_RECEIPTS
            || self.0.keys().any(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err("Runtime write observer identity or bounds invalid".into());
        }
        Ok(())
    }
}
impl UserSendStore {
    pub(crate) fn admit_runtime_envelope(
        &self,
        reservation: &UserReservation,
        input: &str,
        observer: Option<DeliveryWriteObserver>,
    ) -> Result<UserSendRecord, String> {
        self.admit_with_question(
            reservation,
            input,
            input,
            None,
            None,
            false,
            RuntimeAdmission {
                observer,
                ..Default::default()
            },
        )
    }
    pub(crate) fn take_runtime_write_observer(
        &self,
        status: &ChatSendStatus,
    ) -> Result<Option<DeliveryWriteObserver>, String> {
        let tab = self.tab(&status.tab_id);
        let mut data = tab.data.lock();
        let loaded = self.load(&tab, &mut data)?;
        let mut next = loaded.index.clone();
        let observer = next
            .runtime_observers
            .consume(&status.send_id, status.state.clone());
        if observer.is_some() {
            self.persist(&tab, &next)?;
            loaded.index = next;
        }
        Ok(observer)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_envelope_claim_keeps_observer_until_exact_status_and_consumes_it_across_restart() {
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
        let observer = DeliveryWriteObserver::PlanReminder {
            plan_id: Some("plan".into()),
            tab_id: "tab".into(),
            reminder_kind: "mode_change".into(),
            state_key: Some("state".into()),
            complete_wrapup_plan_id: Some("plan".into()),
        };
        let record = store
            .admit_runtime_envelope(&reservation, "opaque", Some(observer.clone()))
            .unwrap();
        let status = ledger.admit_user(&record);
        drop(reservation);
        let reopened = UserSendStore::open(dir.path()).unwrap();
        assert_eq!(reopened.take_runtime_write_observer(&status).unwrap(), None);
        let sent = ledger.confirm_send(&record.send_id).unwrap();
        assert_eq!(
            reopened.take_runtime_write_observer(&sent).unwrap(),
            Some(observer)
        );
        let reopened = UserSendStore::open(dir.path()).unwrap();
        assert_eq!(reopened.take_runtime_write_observer(&sent).unwrap(), None);
    }
    #[test]
    fn pending_to_written_consumes_exact_observer_once_and_late_proof_survives_restore() {
        let mut observers = WriteObservers::default();
        let id = uuid::Uuid::new_v4().to_string();
        let observer = DeliveryWriteObserver::PlanReminder {
            plan_id: None,
            tab_id: "tab".into(),
            reminder_kind: "opaque".into(),
            state_key: Some("native-state".into()),
            complete_wrapup_plan_id: None,
        };
        observers.insert(&[], &[], &id, observer.clone()).unwrap();
        assert!(observers.consume(&id, ChatSendState::Pending).is_none());
        assert!(observers.consume(&id, ChatSendState::Unknown).is_none());
        let mut restored: WriteObservers =
            serde_json::from_str(&serde_json::to_string(&observers).unwrap()).unwrap();
        assert_eq!(restored.consume(&id, ChatSendState::Sent), Some(observer));
        assert!(restored.consume(&id, ChatSendState::Written).is_none());
        let mut restored: WriteObservers =
            serde_json::from_str(&serde_json::to_string(&restored).unwrap()).unwrap();
        assert!(restored.consume(&id, ChatSendState::Sent).is_none());
        let written_id = uuid::Uuid::new_v4().to_string();
        let observer = DeliveryWriteObserver::PlanRole {
            plan_id: "plan".into(),
            role: "role".into(),
        };
        restored
            .insert(&[], &[], &written_id, observer.clone())
            .unwrap();
        assert_eq!(
            restored.consume(&written_id, ChatSendState::Written),
            Some(observer)
        );
        assert!(restored.consume(&written_id, ChatSendState::Sent).is_none());
    }
}
