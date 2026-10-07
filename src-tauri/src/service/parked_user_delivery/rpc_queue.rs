use super::*;
use crate::service::codex_rpc::{lifecycle, send};

impl crate::service::ServiceContext {
    pub(super) fn drain_rpc_user_queue(
        &self,
        head: &UserSendRecord,
    ) -> Option<Result<UserDrain, String>> {
        let tab = &head.authority.tab_id;
        if lifecycle::delivery_state(tab) != lifecycle::DeliveryState::Ready
            || !super::super::codex_rpc::launch::requested(self, &head.authority.provider_id)
        {
            return None;
        }
        Some(self.submit_rpc_user_queue(head))
    }

    fn submit_rpc_user_queue(&self, head: &UserSendRecord) -> Result<UserDrain, String> {
        let tab = &head.authority.tab_id;
        let store = &self.pending_delivery_payloads.user_sends;
        if send::user_turn_pending(
            super::super::codex_rpc::control::active_turn(tab).is_some(),
            lifecycle::for_tab(tab).is_some_and(|sidecar| sidecar.busy.load(Ordering::Acquire)),
        ) {
            return Ok(self.pending_user_send(head, "active_rpc_turn"));
        }
        let records = store
            .queued(tab)?
            .into_iter()
            .take_while(|record| record.authority == head.authority && store.eligible(record))
            .collect::<Vec<_>>();
        if records
            .first()
            .is_none_or(|record| record.send_id != head.send_id)
        {
            return Ok(self.pending_user_send(head, "rpc_queue_head_changed"));
        }
        let texts = records
            .iter()
            .map(|record| {
                store.prepared(record).map(|text| {
                    format!(
                        "{text}{}",
                        record.typed_follow_up.as_deref().unwrap_or_default()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let text = send::joined_text(texts);
        let evidence = self.chat_send_anchor(tab);
        let Some(session) = evidence
            .live_binding
            .as_ref()
            .and_then(|binding| binding.session_id.clone())
        else {
            return Ok(self.pending_user_send(head, "rpc_queue_binding_pending"));
        };
        if evidence
            .live_binding
            .as_ref()
            .and_then(|binding| binding.binding_nonce.as_deref())
            != Some(head.authority.binding_nonce.as_str())
            || !self.user_authority_is_ready(&head.authority)
        {
            return Ok(self.pending_user_send(head, "rpc_queue_authority_changed"));
        }
        if !store.claim_rpc_group(&records, &session)? {
            return Ok(self.pending_user_send(head, "rpc_queue_claim_refused"));
        }
        let mut begun = true;
        for record in &records {
            if !self
                .chat_send_ledger
                .begin_user_handoff(record, &text, self.chat_send_anchor(tab))
            {
                begun = false;
                break;
            }
            self.clear_user_pending_reason(record);
        }
        let ids = records
            .iter()
            .map(|record| record.send_id.as_str())
            .collect::<Vec<_>>();
        let dispatch_generation = store.tab(tab).dispatch_generation.load(Ordering::Acquire);
        let outcome = if begun && self.user_authority_is_ready(&head.authority) {
            send::try_send_group(
                self,
                tab,
                &text,
                send::Origin::User,
                Some(&head.send_id),
                &ids,
            )
            .unwrap_or_else(|| {
                Err(DeliveryError::TabBusyRefused {
                    tab: tab.clone(),
                    detail: "RPC route changed before group handoff".into(),
                })
            })
        } else {
            Err(DeliveryError::TabBusyRefused {
                tab: tab.clone(),
                detail: "RPC group authority changed before handoff".into(),
            })
        };
        let zero_write = outcome
            .as_ref()
            .err()
            .is_some_and(|error| !error.bytes_written())
            && ids
                .iter()
                .all(|id| !self.chat_send_ledger.has_rpc_handoff(id));
        for record in &records {
            store.finish_handoff(record, zero_write)?;
            if zero_write {
                self.chat_send_ledger
                    .zero_write_user_handoff(&record.send_id);
            }
            if let Some(status) = self.chat_send_ledger.finish(
                &record.send_id,
                outcome
                    .as_ref()
                    .map(|_| ())
                    .map_err(|error| error.ledger_message()),
            ) {
                crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
            }
            if !zero_write {
                store.complete(record)?;
            }
        }
        if zero_write {
            self.pending_user_send(head, "rpc_queue_payload_not_written");
            Ok(UserDrain::ZeroWriteRefusal {
                dispatch_generation,
            })
        } else {
            Ok(UserDrain::Idle)
        }
    }
}
