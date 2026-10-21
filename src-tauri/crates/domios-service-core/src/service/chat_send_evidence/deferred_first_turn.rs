use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{
    written_payload_match_end, ChatSendStatus, DeferredFirstTurnCandidate,
};
use crate::service::ServiceContext;

struct CurrentSessionBinding {
    provider_id: String,
    cwd: String,
    session_id: String,
    binding_nonce: String,
}

fn current_session_binding(
    svc: &ServiceContext,
    tab_id: &str,
    candidate: &DeferredFirstTurnCandidate,
) -> Option<CurrentSessionBinding> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(tab_id)?;
    if !tab.launch_session_authenticated()
        || tab.agent_type.as_deref() != Some(candidate.evidence.provider_id.as_str())
        || tab.cwd.as_deref().unwrap_or_default() != candidate.evidence.cwd.as_str()
        || tab.session_binding_nonce.as_deref() != Some(candidate.evidence.binding_nonce.as_str())
        || tab.reported_session_binding_nonce.as_deref()
            != Some(candidate.evidence.binding_nonce.as_str())
    {
        return None;
    }
    Some(CurrentSessionBinding {
        provider_id: candidate.evidence.provider_id.clone(),
        cwd: candidate.evidence.cwd.clone(),
        session_id: tab.agent_session_id.clone()?,
        binding_nonce: candidate.evidence.binding_nonce.clone(),
    })
}

fn first_session_user_turn(
    registry: &AgentRegistry,
    binding: &CurrentSessionBinding,
) -> Option<String> {
    let structured = registry.with_provider(&binding.provider_id, |provider| {
        provider.uses_structured_chat()
    })?;
    if structured {
        return registry
            .with_provider(&binding.provider_id, |provider| {
                provider.read_structured_chat(&binding.cwd, &binding.session_id, None)
            })?
            .messages
            .into_iter()
            .find(|message| message.kind == "user")
            .map(|message| message.content);
    }
    let path = registry
        .with_provider(&binding.provider_id, |provider| {
            provider.session_jsonl_path(&binding.cwd, &binding.session_id)
        })
        .flatten()?;
    let delta = registry.with_provider(&binding.provider_id, |provider| {
        provider.parse_session_delta(&path, 0)
    })?;
    delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(index, message)| crate::service::tab_chat::rich_to_chat_msgs(message, index))
        .find(|message| message.kind == "user")
        .map(|message| message.content)
}

fn first_turn_matches(candidate: &DeferredFirstTurnCandidate, turn: &str) -> bool {
    written_payload_match_end(turn, &candidate.payload, candidate.turn_normalization, 0).is_some()
}

pub(super) fn reconcile(svc: &ServiceContext, tab_id: &str) -> Vec<(ChatSendStatus, String)> {
    let mut confirmed = Vec::new();
    for candidate in svc.chat_send_ledger.deferred_first_turn_candidates(tab_id) {
        let Some(binding) = current_session_binding(svc, tab_id, &candidate) else {
            continue;
        };
        let starts_on_first_turn = svc
            .agent_registry
            .with_provider(&binding.provider_id, |provider| {
                provider.session_starts_on_first_user_turn()
            })
            .unwrap_or(false);
        if !starts_on_first_turn {
            continue;
        }
        let Some(turn) = first_session_user_turn(&svc.agent_registry, &binding) else {
            continue;
        };
        if !first_turn_matches(&candidate, &turn)
            || current_session_binding(svc, tab_id, &candidate).is_none_or(|current| {
                current.session_id != binding.session_id
                    || current.binding_nonce != binding.binding_nonce
            })
        {
            continue;
        }
        let Some(status) = svc
            .chat_send_ledger
            .confirm_deferred_first_turn(&candidate, &binding.session_id)
        else {
            continue;
        };
        confirmed.push((status, binding.session_id));
    }
    confirmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::TurnEvidenceNormalization;
    use crate::service::chat_send_transaction::DeferredFirstTurnEvidence;

    #[test]
    fn first_turn_applies_only_the_candidates_declared_normalization() {
        let mut candidate = DeferredFirstTurnCandidate {
            send_id: "send".into(),
            payload: "opaque 🚀 whole payload".into(),
            turn_normalization: Default::default(),
            evidence: DeferredFirstTurnEvidence {
                provider_id: "capability-fixture".into(),
                cwd: "cwd".into(),
                binding_nonce: "launch".into(),
            },
        };
        let turn = "earlier\nopaque  whole payload\nother";
        assert!(!first_turn_matches(&candidate, turn));
        candidate.turn_normalization = TurnEvidenceNormalization {
            drop_supplementary_characters: true,
            ..Default::default()
        };
        assert!(first_turn_matches(&candidate, turn));
        assert!(!first_turn_matches(&candidate, "opaque  whole"));
        candidate.payload = "🚀".into();
        assert!(!first_turn_matches(&candidate, turn));
    }
}
