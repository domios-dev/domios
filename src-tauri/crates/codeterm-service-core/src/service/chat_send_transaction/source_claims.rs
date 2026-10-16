use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRowRef {
    pub source_session_id: String,
    pub record: SourceRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRecord {
    Jsonl { record_start: u64, record_row: u32 },
    Structured { thread_id: String, item_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MatchedRow {
    pub source: SourceRowRef,
    pub ordinal: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SendRowClaim {
    pub(crate) send_id: String,
    pub(crate) tab_id: String,
    pub(crate) client_token: Option<String>,
    pub(crate) ledger_sequence: u64,
    pub(crate) matched_row: MatchedRow,
}

impl SendRowClaim {
    pub(crate) fn from_status(status: &ChatSendStatus) -> Option<Self> {
        Some(Self {
            send_id: status.send_id.clone(),
            tab_id: status.tab_id.clone(),
            client_token: status.correlation_id.clone(),
            ledger_sequence: status.sequence,
            matched_row: status.matched_row.clone()?,
        })
    }
}

impl SourceRowRef {
    pub(crate) fn jsonl(session: &str, position: TurnPosition) -> Option<Self> {
        Some(Self {
            source_session_id: session.into(),
            record: SourceRecord::Jsonl {
                record_start: position.record_start?,
                record_row: position.record_row,
            },
        })
    }

    pub(crate) fn structured(session: &str, id: &str) -> Option<Self> {
        (!id.is_empty()).then(|| Self {
            source_session_id: session.into(),
            record: SourceRecord::Structured {
                thread_id: session.into(),
                item_id: id.into(),
            },
        })
    }

    pub(crate) fn valid(&self) -> bool {
        !self.source_session_id.is_empty()
            && match &self.record {
                SourceRecord::Jsonl { .. } => true,
                SourceRecord::Structured { thread_id, item_id } => {
                    thread_id == &self.source_session_id && !item_id.is_empty()
                }
            }
    }
}

impl ChatSendLedger {
    pub(super) fn claim_rows_locked(
        state: &mut ChatSendLedgerState,
        sends: &[String],
        source: &SourceRowRef,
    ) -> bool {
        if !source.valid() || sends.is_empty() || sends.len() > u32::MAX as usize {
            return false;
        }
        if state.by_id.values().any(|status| {
            status
                .matched_row
                .as_ref()
                .is_some_and(|row| row.source == *source)
        }) {
            return false;
        }
        let mut unique = HashSet::new();
        if sends.iter().any(|id| {
            !unique.insert(id)
                || state
                    .by_id
                    .get(id)
                    .is_none_or(|status| status.matched_row.is_some())
                || state
                    .live_binding_by_id
                    .get(id)
                    .and_then(|binding| binding.session_id.as_deref())
                    .is_some_and(|session| session != source.source_session_id)
                || state
                    .anchor_by_id
                    .get(id)
                    .is_some_and(|anchor| anchor.session_id != source.source_session_id)
        }) {
            return false;
        }
        for (index, id) in sends.iter().enumerate() {
            if let Some(status) = state.by_id.get_mut(id) {
                status.matched_row = Some(MatchedRow {
                    source: source.clone(),
                    ordinal: index as u32 + 1,
                });
            }
        }
        true
    }

    pub(crate) fn source_row_claimed(&self, source: &SourceRowRef) -> bool {
        self.state.lock().by_id.values().any(|status| {
            status
                .matched_row
                .as_ref()
                .is_some_and(|row| row.source == *source)
        })
    }

    pub(super) fn persist_source_claims(&self, statuses: &[ChatSendStatus]) {
        let claims: Vec<_> = statuses
            .iter()
            .filter_map(SendRowClaim::from_status)
            .collect();
        let Some(tab) = claims.first().map(|claim| claim.tab_id.clone()) else {
            return;
        };
        let owner = self.owner.lock().as_ref().and_then(Weak::upgrade);
        if let Some(owner) = owner {
            if let Err(error) = owner
                .pending_delivery_payloads
                .user_sends
                .record_source_claims(&tab, &claims)
            {
                log::warn!("[chat-send] source claim persistence tab={tab} failed: {error}");
            }
        }
    }

    pub(crate) fn restore_source_claim(&self, claim: &SendRowClaim) {
        if !claim.matched_row.source.valid() || claim.matched_row.ordinal == 0 {
            return;
        }
        {
            let state = self.state.lock();
            if state.by_id.get(&claim.send_id).is_some_and(|status| {
                status.tab_id != claim.tab_id
                    || status.correlation_id != claim.client_token
                    || status
                        .matched_row
                        .as_ref()
                        .is_some_and(|row| row != &claim.matched_row)
            }) {
                return;
            }
            if state
                .live_binding_by_id
                .get(&claim.send_id)
                .and_then(|binding| binding.session_id.as_deref())
                .is_some_and(|session| session != claim.matched_row.source.source_session_id)
                || state
                    .anchor_by_id
                    .get(&claim.send_id)
                    .is_some_and(|anchor| {
                        anchor.session_id != claim.matched_row.source.source_session_id
                    })
            {
                return;
            }
        }
        let receipt = super::super::pending_delivery_payloads::user_sends::continuity::UserSendReceipt {
            send_id: claim.send_id.clone(), tab_id: claim.tab_id.clone(),
            client_token: claim.client_token.clone(), ledger_sequence: claim.ledger_sequence,
            state: super::super::pending_delivery_payloads::user_sends::continuity::ReceiptState::Delivered,
        };
        self.restore_user_receipt(&receipt);
        let mut state = self.state.lock();
        if state.by_id.values().any(|status| {
            status.send_id != claim.send_id
                && status.matched_row.as_ref() == Some(&claim.matched_row)
        }) {
            return;
        }
        if state
            .by_id
            .get(&claim.send_id)
            .is_some_and(|status| status.state != ChatSendState::Sent)
        {
            Self::mark_sent(&mut state, &claim.send_id);
        }
        if let Some(status) = state.by_id.get_mut(&claim.send_id) {
            if status.tab_id == claim.tab_id && status.correlation_id == claim.client_token {
                status.matched_row = Some(claim.matched_row.clone());
            }
        }
    }
}

pub(crate) fn delivered_turn_confirmations(
    ledger: &ChatSendLedger,
    tab_id: &str,
    source_session_id: &str,
    source_binding_nonce: Option<&str>,
    messages: &[crate::agent::chat_types::ChatMsg],
    provenance: Option<&LiveProvenance>,
) -> Vec<ChatSendStatus> {
    if let Some(provenance) = provenance {
        ledger.advance_live_evidence(tab_id, source_session_id, source_binding_nonce, provenance);
    }
    let mut confirmed = Vec::new();
    for (index, msg) in messages.iter().enumerate() {
        if msg.kind != "user" {
            continue;
        }
        let positioned = provenance.and_then(|provenance| {
            provenance.positions.get(index).copied().map(|position| {
                ledger.confirm_owned_live_turns(
                    tab_id,
                    source_session_id,
                    source_binding_nonce,
                    msg,
                    position,
                    provenance,
                )
            })
        });
        let source_row = if let Some(provenance) = provenance {
            provenance
                .positions
                .get(index)
                .and_then(|position| SourceRowRef::jsonl(source_session_id, *position))
        } else {
            SourceRowRef::structured(source_session_id, &msg.id)
        };
        let statuses = positioned
            .filter(|statuses| !statuses.is_empty())
            .unwrap_or_else(|| {
                ledger.confirm_user_turn_with_row(
                    tab_id,
                    source_session_id,
                    source_binding_nonce,
                    &msg.content,
                    source_row.as_ref(),
                )
            });
        for status in statuses {
            log::info!(
                "[chat-send] send_id={} tab={} session={} confirmed by exact turn evidence",
                status.send_id,
                status.tab_id,
                source_session_id
            );
            confirmed.push(status);
        }
    }
    confirmed
}

#[cfg(test)]
mod tests;
