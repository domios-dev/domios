mod parked_order;
use super::{
    matching_turn_send_ids, ChatSendLedger, ChatSendState, ChatSendStatus, SendEvidenceAnchor,
};
use crate::service::pending_delivery_payloads::user_sends::UserSendAuthority;

fn append_key_part(key: &mut String, part: &str) {
    key.push_str(&part.len().to_string());
    key.push(':');
    key.push_str(part);
}

fn merged_turn_evidence_key(tab_id: &str, anchor: &SendEvidenceAnchor, turn_text: &str) -> String {
    let mut key = String::new();
    let normalization = format!("{:?}", anchor.turn_normalization);
    for part in [
        tab_id,
        anchor.provider_id.as_str(),
        anchor.cwd.as_str(),
        anchor.session_id.as_str(),
        normalization.as_str(),
    ] {
        append_key_part(&mut key, part);
    }
    match &anchor.window {
        super::EvidenceWindow::Jsonl { transcript_offset } => {
            append_key_part(&mut key, "jsonl");
            append_key_part(&mut key, &transcript_offset.to_string());
        }
        super::EvidenceWindow::Structured { cursor } => {
            append_key_part(&mut key, "structured");
            append_key_part(&mut key, cursor.as_deref().unwrap_or(""));
            append_key_part(
                &mut key,
                if cursor.is_some() {
                    "present"
                } else {
                    "absent"
                },
            );
        }
    }
    append_key_part(&mut key, turn_text);
    key
}

impl ChatSendLedger {
    pub(crate) fn evidence_session_for_send(&self, send_id: &str) -> Option<String> {
        let state = self.state.lock();
        state
            .live_binding_by_id
            .get(send_id)
            .and_then(|binding| binding.session_id.clone())
            .or_else(|| {
                state
                    .anchor_by_id
                    .get(send_id)
                    .map(|anchor| anchor.session_id.clone())
            })
    }
    pub(crate) fn session_conversation(&self, tab_id: &str, session_id: &str) -> Option<bool> {
        let state = self.state.lock();
        let mut uncertain = false;
        for status in state
            .by_id
            .values()
            .filter(|status| status.tab_id == tab_id)
        {
            let binding = state
                .live_binding_by_id
                .get(&status.send_id)
                .and_then(|binding| binding.session_id.as_deref())
                .or_else(|| {
                    state
                        .anchor_by_id
                        .get(&status.send_id)
                        .map(|anchor| anchor.session_id.as_str())
                });
            if binding.is_some_and(|binding| binding != session_id) {
                continue;
            }
            match status.state {
                ChatSendState::Sent => return Some(true),
                ChatSendState::Written | ChatSendState::Unknown => uncertain = true,
                _ => {}
            }
        }
        (!uncertain).then_some(false)
    }
    pub(crate) fn has_written_user_send_before(
        &self,
        authority: &UserSendAuthority,
        sequence: u64,
        held: bool,
    ) -> bool {
        let state = self.state.lock();
        state.owned.iter().any(|(id, (prior, _))| {
            !state.rpc_by_id.contains_key(id)
                && state.by_id.get(id).is_some_and(|status| {
                    parked_order::blocks_later(
                        held,
                        prior.tab_id == authority.tab_id
                            && prior.pty_id == authority.pty_id
                            && prior.binding_nonce == authority.binding_nonce
                            && prior.pty_session_generation == authority.pty_session_generation
                            && prior.provider_id == authority.provider_id,
                        status.sequence < sequence,
                        &status.state,
                    )
                })
        })
    }

    pub(crate) fn has_unsettled_user_send_before(
        &self,
        authority: &UserSendAuthority,
        sequence: u64,
    ) -> bool {
        let state = self.state.lock();
        state.order.iter().any(|send_id| {
            let Some(status) = state.by_id.get(send_id) else {
                return false;
            };
            let Some((prior, _)) = state.owned.get(send_id) else {
                return false;
            };
            let same_launch = prior.tab_id == authority.tab_id
                && prior.pty_id == authority.pty_id
                && prior.binding_nonce == authority.binding_nonce
                && prior.pty_session_generation == authority.pty_session_generation
                && prior.provider_id == authority.provider_id;
            same_launch
                && status.sequence < sequence
                && matches!(
                    status.state,
                    ChatSendState::Written | ChatSendState::Unknown
                )
        })
    }

    pub(crate) fn has_held_user_send_before(
        &self,
        authority: &UserSendAuthority,
        sequence: u64,
    ) -> bool {
        let state = self.state.lock();
        state.order.iter().any(|send_id| {
            let Some(status) = state.by_id.get(send_id) else {
                return false;
            };
            state.held_before_readiness_by_id.get(send_id) == Some(authority)
                && status.tab_id == authority.tab_id
                && status.sequence < sequence
                && matches!(
                    status.state,
                    ChatSendState::Written | ChatSendState::Unknown | ChatSendState::Sent
                )
        })
    }

    #[cfg(test)]
    pub(crate) fn confirm_delivered_user_turn(
        &self,
        tab_id: &str,
        source_session_id: &str,
        turn_text: &str,
    ) -> Option<ChatSendStatus> {
        self.confirm_delivered_user_turn_scoped(tab_id, source_session_id, None, turn_text)
    }

    #[cfg(test)]
    pub(crate) fn confirm_delivered_user_turn_scoped(
        &self,
        tab_id: &str,
        source_session_id: &str,
        source_binding_nonce: Option<&str>,
        turn_text: &str,
    ) -> Option<ChatSendStatus> {
        self.confirm_delivered_user_turns_scoped(
            tab_id,
            source_session_id,
            source_binding_nonce,
            turn_text,
        )
        .into_iter()
        .next()
    }

    pub(crate) fn confirm_delivered_user_turns_scoped(
        &self,
        tab_id: &str,
        source_session_id: &str,
        source_binding_nonce: Option<&str>,
        turn_text: &str,
    ) -> Vec<ChatSendStatus> {
        let owned_wake = {
            let state = self.state.lock();
            state.owned.iter().any(|(id, (_, active))| {
                *active
                    && state
                        .by_id
                        .get(id)
                        .is_some_and(|status| status.tab_id == tab_id)
                    && state.live_binding_by_id.get(id).is_some_and(|binding| {
                        binding.session_id.as_deref() == Some(source_session_id)
                            && binding.binding_nonce.as_deref() == source_binding_nonce
                    })
            })
        };
        if owned_wake {
            self.wake_owner(tab_id);
        }
        let candidates = {
            let state = self.state.lock();
            state
                .order
                .iter()
                .filter_map(|send_id| {
                    let status = state.by_id.get(send_id)?;
                    let binding_matches =
                        state.live_binding_by_id.get(send_id).is_none_or(|binding| {
                            binding
                                .session_id
                                .as_deref()
                                .is_none_or(|session_id| session_id == source_session_id)
                                && binding
                                    .binding_nonce
                                    .as_deref()
                                    .is_none_or(|nonce| Some(nonce) == source_binding_nonce)
                        });
                    if state.owned.contains_key(send_id)
                        || state.terminal_failures.contains(send_id)
                        || status.tab_id != tab_id
                        || status.state == ChatSendState::Sent
                        || !binding_matches
                    {
                        return None;
                    }
                    Some((
                        send_id.clone(),
                        state.payload_by_id.get(send_id)?.clone(),
                        state
                            .turn_normalization_by_id
                            .get(send_id)
                            .copied()
                            .unwrap_or_default(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        let matching_ids = matching_turn_send_ids(turn_text, &candidates);
        if matching_ids.is_empty() {
            return Vec::new();
        }
        let mut state = self.state.lock();
        let still_eligible = matching_ids.iter().all(|send_id| {
            !state.terminal_failures.contains(send_id)
                && !state.owned.contains_key(send_id)
                && state.by_id.get(send_id).is_some_and(|status| {
                    status.tab_id == tab_id && status.state != ChatSendState::Sent
                })
                && state.live_binding_by_id.get(send_id).is_none_or(|binding| {
                    binding
                        .session_id
                        .as_deref()
                        .is_none_or(|session_id| session_id == source_session_id)
                        && binding
                            .binding_nonce
                            .as_deref()
                            .is_none_or(|nonce| Some(nonce) == source_binding_nonce)
                })
        });
        if !still_eligible {
            return Vec::new();
        }
        let mut statuses = Vec::with_capacity(matching_ids.len());
        for send_id in &matching_ids {
            if let Some(status) = Self::mark_sent(&mut state, send_id) {
                statuses.push(Self::with_retention_window(&state, status));
            }
        }
        drop(state);
        if owned_wake {
            self.wake_owner(tab_id);
        }
        statuses
    }

    pub(crate) fn confirm_merged_evidence_turn(
        &self,
        tab_id: &str,
        anchor: &SendEvidenceAnchor,
        turn_text: &str,
        eligible_send_ids: &[String],
    ) -> Vec<ChatSendStatus> {
        let candidates = {
            let state = self.state.lock();
            state
                .order
                .iter()
                .filter_map(|send_id| {
                    let status = state.by_id.get(send_id)?;
                    let candidate_anchor = state.anchor_by_id.get(send_id)?;
                    let same_source = candidate_anchor.provider_id == anchor.provider_id
                        && candidate_anchor.cwd == anchor.cwd
                        && candidate_anchor.session_id == anchor.session_id
                        && candidate_anchor.turn_normalization == anchor.turn_normalization;
                    let active_owned = state.owned.get(send_id).is_some_and(|(_, active)| *active);
                    if status.tab_id != tab_id
                        || status.state == ChatSendState::Sent
                        || state.terminal_failures.contains(send_id)
                        || !same_source
                        || candidate_anchor != anchor && !eligible_send_ids.contains(send_id)
                        || state.owned.contains_key(send_id) && !active_owned
                        || active_owned
                            && !matches!(
                                status.state,
                                ChatSendState::Written | ChatSendState::Unknown
                            )
                    {
                        return None;
                    }
                    let owned = state.owned.get(send_id).cloned();
                    let binding = state.live_binding_by_id.get(send_id).cloned();
                    Some((
                        send_id.clone(),
                        state.payload_by_id.get(send_id)?.clone(),
                        candidate_anchor.clone(),
                        candidate_anchor.turn_normalization,
                        owned,
                        binding,
                    ))
                })
                .collect::<Vec<_>>()
        };
        let matching = matching_turn_send_ids(
            turn_text,
            &candidates
                .iter()
                .map(|(send_id, payload, _, normalization, _, _)| {
                    (send_id.clone(), payload.clone(), *normalization)
                })
                .collect::<Vec<_>>(),
        );
        if matching.len() < 2 {
            return Vec::new();
        }
        let owner = self.owner.lock().as_ref().and_then(|weak| weak.upgrade());
        let matching_candidates = matching
            .iter()
            .filter_map(|send_id| {
                candidates
                    .iter()
                    .find(|(candidate_id, _, _, _, _, _)| candidate_id == send_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        if matching_candidates.len() != matching.len()
            || matching_candidates
                .iter()
                .any(|(_, _, _, _, owned, binding)| {
                    let Some((authority, active)) = owned else {
                        return false;
                    };
                    if !*active {
                        return true;
                    }
                    let binding_matches = binding.as_ref().is_none_or(|binding| {
                        binding
                            .session_id
                            .as_deref()
                            .is_none_or(|session| session == anchor.session_id)
                            && binding
                                .binding_nonce
                                .as_deref()
                                .is_none_or(|nonce| nonce == authority.binding_nonce)
                    });
                    !binding_matches
                        || authority.provider_id != anchor.provider_id
                        || owner.as_ref().is_none_or(|owner| {
                            !owner.user_authority_is_current_session(
                                authority,
                                &anchor.session_id,
                                &authority.binding_nonce,
                            )
                        })
                })
        {
            return Vec::new();
        }
        let mut state = self.state.lock();
        let still_eligible =
            matching_candidates
                .iter()
                .all(|(send_id, _, anchor_snapshot, _, owned, binding)| {
                    let current_anchor = state.anchor_by_id.get(send_id);
                    let current_owned = state.owned.get(send_id);
                    current_anchor == Some(anchor_snapshot)
                        && anchor_snapshot.provider_id == anchor.provider_id
                        && anchor_snapshot.cwd == anchor.cwd
                        && anchor_snapshot.session_id == anchor.session_id
                        && anchor_snapshot.turn_normalization == anchor.turn_normalization
                        && (anchor_snapshot == anchor || eligible_send_ids.contains(send_id))
                        && !state.terminal_failures.contains(send_id)
                        && state.by_id.get(send_id).is_some_and(|status| {
                            status.tab_id == tab_id
                                && status.state != ChatSendState::Sent
                                && (current_owned.is_none()
                                    || current_owned.is_some_and(|(_, active)| *active)
                                        && matches!(
                                            status.state,
                                            ChatSendState::Written | ChatSendState::Unknown
                                        ))
                        })
                        && current_owned == owned.as_ref()
                        && state.live_binding_by_id.get(send_id) == binding.as_ref()
                });
        if !still_eligible {
            return Vec::new();
        }
        let evidence_key = merged_turn_evidence_key(tab_id, anchor, turn_text);
        let mut statuses = Vec::with_capacity(matching.len());
        for send_id in &matching {
            if let Some(status) = Self::mark_sent(&mut state, send_id) {
                statuses.push(Self::with_retention_window(&state, status));
            }
        }
        if statuses.len() == matching.len() {
            if !state.consumed_merged_turns.contains_key(&evidence_key) {
                if state.consumed_merged_turn_order.len() >= super::MAX_RESULTS {
                    if let Some(expired) = state.consumed_merged_turn_order.pop_front() {
                        state.consumed_merged_turns.remove(&expired);
                    }
                }
                state
                    .consumed_merged_turn_order
                    .push_back(evidence_key.clone());
            }
            *state.consumed_merged_turns.entry(evidence_key).or_default() += 1;
        }
        drop(state);
        self.wake_owner(tab_id);
        statuses
    }

    pub(crate) fn consumed_merged_turn_count(
        &self,
        tab_id: &str,
        anchor: &SendEvidenceAnchor,
        turn_text: &str,
    ) -> usize {
        let key = merged_turn_evidence_key(tab_id, anchor, turn_text);
        self.state
            .lock()
            .consumed_merged_turns
            .get(&key)
            .copied()
            .unwrap_or(0)
    }
}
