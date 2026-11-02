use super::*;
use crate::service::chat_send_evidence::{contains_exact_user_turn_since, ExactTurnCursor};
use crate::service::ServiceContext;

impl ChatSendLedger {
    pub(crate) fn reconcile_provisional_launch(&self, tab_id: &str, source: Option<(&str, &str)>) {
        let Some(owner) = self.owner.lock().as_ref().and_then(Weak::upgrade) else {
            return;
        };
        for candidate in self.evidence_candidates(tab_id) {
            let authority = {
                let state = self.state.lock();
                state
                    .owned
                    .get(&candidate.send_id)
                    .and_then(|(authority, active)| {
                        (*active
                            && state
                                .live_binding_by_id
                                .get(&candidate.send_id)
                                .is_some_and(|binding| {
                                    binding.session_id.as_deref()
                                        == Some(candidate.anchor.session_id.as_str())
                                        && binding.binding_nonce.as_deref()
                                            == Some(authority.binding_nonce.as_str())
                                }))
                        .then(|| authority.clone())
                    })
            };
            let Some(authority) = authority else { continue };
            if source.is_some_and(|(session, nonce)| {
                session != candidate.anchor.session_id || nonce != authority.binding_nonce
            }) || !owner.provisional_send_matches(&authority, &candidate.anchor)
            {
                continue;
            }
            if !contains_exact_user_turn_since(
                &owner.agent_registry,
                &candidate.anchor,
                &candidate.payload,
                &mut ExactTurnCursor::for_anchor(Some(&candidate.anchor)),
            ) {
                continue;
            }
            let binding = {
                let mut mux = owner.mux.write();
                let Some(tab) = mux.get_tab_mut(tab_id) else {
                    return;
                };
                if !provisional_tab_matches(tab, &authority, &candidate.anchor) {
                    return;
                }
                let pty = owner.pty.lock();
                if !authority.pty_session_generation.is_some_and(|generation| {
                    pty.session_generation_matches(
                        authority.pty_id.as_deref().unwrap_or_default(),
                        generation,
                    )
                }) {
                    return;
                }
                tab.reported_session_binding_nonce = Some(authority.binding_nonce.clone());
                crate::service::ProviderSessionBinding {
                    pty_id: authority.pty_id.clone().unwrap_or_default(),
                    binding_nonce: authority.binding_nonce.clone(),
                }
            };
            owner.retire_provisional_launch(tab_id, &binding);
            return;
        }
    }
}

fn provisional_tab_matches(
    tab: &crate::multiplexer::types::Tab,
    authority: &UserSendAuthority,
    anchor: &SendEvidenceAnchor,
) -> bool {
    tab.boot_stage.is_some_and(|stage| stage.is_provisional())
        && matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty)
        && tab.pty_id == authority.pty_id
        && tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str())
        && tab.agent_type.as_deref() == Some(anchor.provider_id.as_str())
        && authority.provider_id == anchor.provider_id
        && tab.cwd.as_deref().unwrap_or_default() == anchor.cwd
        && tab.agent_session_id.as_deref() == Some(anchor.session_id.as_str())
        && authority.observed_session_id.as_deref() == Some(anchor.session_id.as_str())
        && tab.provisional_launch.as_ref().is_some_and(|launch| {
            Some(&launch.pty_id) == authority.pty_id.as_ref()
                && launch.binding_nonce == authority.binding_nonce
        })
}

impl ServiceContext {
    fn provisional_send_matches(
        &self,
        authority: &UserSendAuthority,
        anchor: &SendEvidenceAnchor,
    ) -> bool {
        self.pending_delivery_payloads
            .user_sends
            .ready_launch_is_latched(authority)
            && self
                .pending_delivery_payloads
                .user_sends
                .launch_was_dispatched(authority)
            && self
                .mux
                .read()
                .get_tab(&authority.tab_id)
                .is_some_and(|tab| provisional_tab_matches(tab, authority, anchor))
    }

    pub(crate) fn provisional_append_stream_allowed(
        &self,
        tab_id: &str,
        session: &str,
        nonce: &str,
    ) -> bool {
        let Ok(authority) = self.user_send_authority(tab_id) else {
            return false;
        };
        authority.binding_nonce == nonce
            && authority.observed_session_id.as_deref() == Some(session)
            && self
                .pending_delivery_payloads
                .user_sends
                .ready_launch_is_latched(&authority)
            && self
                .pending_delivery_payloads
                .user_sends
                .launch_was_dispatched(&authority)
            && self.mux.read().get_tab(tab_id).is_some_and(|tab| {
                tab.boot_stage.is_some_and(|stage| stage.is_provisional())
                    && tab.provisional_launch.as_ref().is_some_and(|launch| {
                        Some(&launch.pty_id) == authority.pty_id.as_ref()
                            && launch.binding_nonce == nonce
                    })
            })
    }
}
