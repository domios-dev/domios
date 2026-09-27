use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

fn take_parked_user_send(
    pending: &mut Option<PendingDeliveryRecord>,
    record_id: &str,
) -> Option<PendingDeliveryRecord> {
    let record = pending.as_ref()?;
    if record.channel != PendingDeliveryChannel::InteractiveInput
        || record.record_id() != record_id
        || record.state() != PendingDeliveryState::Pending
    {
        return None;
    }
    pending.take()
}

fn return_after_send_failure(
    pending: &mut Option<PendingDeliveryRecord>,
    record: PendingDeliveryRecord,
    bytes_written: bool,
) -> Result<(), PendingDeliveryRecord> {
    if bytes_written
        || pending
            .as_ref()
            .is_some_and(|current| current.record_id() != record.record_id())
    {
        return Err(record);
    }
    *pending = Some(record);
    Ok(())
}

impl super::ServiceContext {
    pub(crate) fn park_refused_user_send(
        &self,
        tab_id: &str,
        send_id: &str,
        text: &str,
        error: &super::delivery_error::DeliveryError,
    ) {
        if error.bytes_written() || text.is_empty() {
            return;
        }
        if let Err(store_error) = self.create_pending_delivery_with_send_id(
            tab_id,
            PendingDeliveryChannel::InteractiveInput,
            text,
            Some(send_id),
        ) {
            log::warn!("[chat-send] Tab {tab_id} refused send could not be parked: {store_error}");
        }
    }

    pub(crate) fn deliver_parked_user_send(&self, tab_id: &str) {
        let parked = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .filter(|record| {
                record.channel == PendingDeliveryChannel::InteractiveInput
                    && record.state() == PendingDeliveryState::Pending
            })
            .map(|record| {
                let delivery_id = record
                    .send_id()
                    .unwrap_or_else(|| record.record_id())
                    .to_string();
                (
                    record.record_id().to_string(),
                    delivery_id,
                    record.content_digest.clone(),
                )
            });
        let Some((record_id, delivery_id, digest)) = parked else {
            return;
        };
        let text = match self.pending_delivery_payloads.resolve(&digest) {
            Ok(text) => text,
            Err(error) => {
                log::warn!(
                    "[chat-send] parked send {} payload could not be resolved: {error}",
                    delivery_id
                );
                return;
            }
        };
        let record = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            take_parked_user_send(&mut tab.pending_delivery, &record_id)
        };
        let Some(record) = record else {
            return;
        };
        if let Err(message) = super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        ) {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                if tab.pending_delivery.is_none() {
                    tab.pending_delivery = Some(record);
                }
            }
            log::warn!(
                "[chat-send] parked send {} claim was not persisted: {message}",
                delivery_id
            );
            return;
        }
        if let Err(error) = self.send_chat_text_claimed(tab_id, &text, None) {
            log::warn!(
                "[chat-send] parked send {} for Tab {tab_id} failed: {}",
                delivery_id,
                error.code()
            );
            if !error.bytes_written() {
                let restored = {
                    let mut mux = self.mux.write();
                    mux.get_tab_mut(tab_id).is_some_and(|tab| {
                        return_after_send_failure(
                            &mut tab.pending_delivery,
                            record,
                            error.bytes_written(),
                        )
                        .is_ok()
                    })
                };
                if restored {
                    if let Err(message) = super::session_persist::persist_session_after_mutation(
                        self,
                        super::session_persist::SessionPersistUrgency::Prompt,
                    ) {
                        log::warn!(
                            "[chat-send] parked send {} retry state was not persisted: {message}",
                            delivery_id
                        );
                    }
                } else {
                    log::error!(
                        "[chat-send] parked send {} pre-write failure could not be restored",
                        delivery_id
                    );
                }
                return;
            }
        }
        let digest_is_live =
            super::pending_delivery_payloads::live_payload_digests(&self.mux.read().snapshot())
                .contains(&record.content_digest);
        if !digest_is_live {
            let mut consumed = Some(record);
            if let Err(error) = self
                .pending_delivery_payloads
                .free_terminal_pair(&mut consumed, "interactive_send_claimed")
            {
                log::warn!(
                    "[chat-send] parked send {} payload cleanup failed: {error}",
                    delivery_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn duplicate_ready_triggers_atomically_claim_one_pending_record_once() {
        let record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into());
        let record_id = record.record_id().to_string();
        let pending = Arc::new(Mutex::new(Some(record)));
        let writes = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(Barrier::new(3));
        let triggers = (0..2)
            .map(|_| {
                let pending = pending.clone();
                let writes = writes.clone();
                let ready = ready.clone();
                let record_id = record_id.clone();
                std::thread::spawn(move || {
                    ready.wait();
                    if take_parked_user_send(&mut pending.lock(), &record_id).is_some() {
                        writes.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect::<Vec<_>>();
        ready.wait();
        for trigger in triggers {
            trigger.join().unwrap();
        }

        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert!(pending.lock().is_none());
    }

    #[test]
    fn parked_send_claim_requires_the_exact_pending_record_id() {
        let mut pending = Some(
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into()),
        );
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert!(take_parked_user_send(&mut pending, "other-record").is_none());
        assert!(take_parked_user_send(&mut pending, &record_id).is_some());
    }

    #[test]
    fn legacy_pending_record_without_send_id_is_deliverable_by_record_id() {
        let legacy: PendingDeliveryRecord = serde_json::from_str(
            r#"{"content_digest":"digest","channel":"interactive_input","state":"pending","attempt_provenance":null}"#,
        )
        .unwrap();
        let mut pending = Some(legacy);
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert_eq!(pending.as_ref().unwrap().send_id(), None);
        assert!(take_parked_user_send(&mut pending, &record_id).is_some());
        assert!(pending.is_none());
    }

    #[test]
    fn prewrite_failure_restores_but_postwrite_failure_does_not() {
        let record =
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput);
        let record_id = record.record_id().to_string();
        let mut pending = None;

        assert!(return_after_send_failure(&mut pending, record, false).is_ok());
        assert_eq!(pending.as_ref().unwrap().record_id(), record_id);

        let claimed = pending.take().unwrap();
        assert!(return_after_send_failure(&mut pending, claimed, true).is_err());
        assert!(pending.is_none());
    }

    #[test]
    fn unconfirmed_send_is_not_claimed_for_another_write() {
        let mut pending = Some(
            PendingDeliveryRecord::new("digest".into(), PendingDeliveryChannel::InteractiveInput)
                .with_send_id("send-1".into()),
        );
        pending.as_mut().unwrap().mark_unconfirmed();
        let record_id = pending.as_ref().unwrap().record_id().to_string();

        assert!(take_parked_user_send(&mut pending, &record_id).is_none());
        assert!(pending.is_some());
    }
}
