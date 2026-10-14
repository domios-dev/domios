use super::*;

impl crate::service::ServiceContext {
    pub(super) fn deliver_parked_bootstrap_record(&self, tab_id: &str) -> Result<(), UserDrain> {
        let dispatch_generation = self
            .pending_delivery_payloads
            .user_sends
            .tab(tab_id)
            .dispatch_generation
            .load(Ordering::Acquire);
        let ready =
            self.mux.read().get_tab(tab_id).is_some_and(|tab| {
                tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
            });
        if !ready {
            return Ok(());
        }
        let parked = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .filter(|record| {
                record.channel == PendingDeliveryChannel::InteractiveInput
                    && record.send_id().is_none()
                    && record.state() == PendingDeliveryState::Pending
            })
            .map(|record| {
                let record_id = record.record_id().to_string();
                let send_id = record.send_id().map(str::to_string);
                let delivery_id = send_id.as_deref().unwrap_or(&record_id).to_string();
                (
                    record_id,
                    send_id,
                    delivery_id,
                    record.content_digest.clone(),
                )
            });
        let Some((record_id, send_id, delivery_id, digest)) = parked else {
            return Ok(());
        };
        let text = match self.pending_delivery_payloads.resolve(&digest) {
            Ok(text) => text,
            Err(error) => {
                log::warn!(
                    "[chat-send] parked send {} payload could not be resolved: {error}",
                    delivery_id
                );
                return Err(UserDrain::ZeroWriteRefusal {
                    dispatch_generation,
                });
            }
        };
        let record = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return Ok(());
            };
            if tab.boot_stage != Some(codeterm_types::pty::BootStage::AgentReady)
                || self
                    .pending_delivery_payloads
                    .user_sends
                    .tab(tab_id)
                    .revoked
                    .load(Ordering::Acquire)
            {
                return Ok(());
            }
            take_parked_user_send(&mut tab.pending_delivery, &record_id, send_id.as_deref())
        };
        let Some(record) = record else {
            return Ok(());
        };
        if let Err(message) = crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
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
            return Err(UserDrain::ZeroWriteRefusal {
                dispatch_generation,
            });
        }
        let dispatch_generation = self
            .pending_delivery_payloads
            .user_sends
            .tab(tab_id)
            .dispatch_generation
            .load(Ordering::Acquire);
        if self
            .pending_delivery_payloads
            .user_sends
            .tab(tab_id)
            .revoked
            .load(Ordering::Acquire)
        {
            return self.settle_parked_bootstrap(
                tab_id,
                record,
                &delivery_id,
                dispatch_generation,
                Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "bootstrap owner revoked before handoff".into(),
                }),
            );
        }
        let service = self.clone();
        let completion_service = self.clone();
        let submit_tab = tab_id.to_owned();
        let completion_tab = tab_id.to_owned();
        let completion_record = record.clone();
        let completion_id = delivery_id.clone();
        let outcome = self.run_write_handoff(
            tab_id,
            move |enqueued| service.submit_parked_bootstrap(&submit_tab, &text, enqueued),
            move |outcome| {
                let _ = completion_service.settle_parked_bootstrap(
                    &completion_tab,
                    completion_record,
                    &completion_id,
                    dispatch_generation,
                    outcome,
                );
            },
        );
        if matches!(outcome, Ok(UserWrite::Enqueued)) {
            return Ok(());
        }
        self.settle_parked_bootstrap(
            tab_id,
            record,
            &delivery_id,
            dispatch_generation,
            outcome.map(|_| ()),
        )
    }

    fn settle_parked_bootstrap(
        &self,
        tab_id: &str,
        record: PendingDeliveryRecord,
        delivery_id: &str,
        dispatch_generation: u64,
        outcome: Result<(), DeliveryError>,
    ) -> Result<(), UserDrain> {
        if let Err(error) = outcome {
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
                    if let Err(message) =
                        crate::service::session_persist::persist_session_after_mutation(
                            self,
                            crate::service::session_persist::SessionPersistUrgency::Prompt,
                        )
                    {
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
                return Err(UserDrain::ZeroWriteRefusal {
                    dispatch_generation,
                });
            }
        }
        let digest_is_live = crate::service::pending_delivery_payloads::live_payload_digests(
            &self.mux.read().snapshot(),
        )
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
        Ok(())
    }

    pub(super) fn submit_parked_bootstrap(
        &self,
        tab_id: &str,
        text: &str,
        enqueued: PayloadEnqueued,
    ) -> Result<(), DeliveryError> {
        #[cfg(test)]
        if let Some(boundary) = tests::queued_boundary::boundary(tab_id) {
            return boundary(None, text, enqueued);
        }
        #[cfg(test)]
        if let Some(boundary) = tests::bootstrap_boundary(tab_id) {
            let outcome = boundary(text);
            if outcome
                .as_ref()
                .err()
                .is_none_or(DeliveryError::bytes_written)
            {
                enqueued();
            }
            return outcome;
        }
        self.send_chat_text_with_enqueued(tab_id, text, enqueued)
    }
}
