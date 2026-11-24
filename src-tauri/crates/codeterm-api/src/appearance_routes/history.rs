use super::{AppConfig, AppearanceProposal, AppearanceState, Refusal, StatusCode};

const PROPOSAL_LIMIT: usize = 12;
const REMOTE_PENDING_PER_SENDER: usize = 3;

pub(super) fn retain(config: &mut AppConfig, proposal: AppearanceProposal) -> Result<(), Refusal> {
    let history = &config.appearance_proposals;
    let disposable_remote = |item: &AppearanceProposal| {
        item.proposed_by_installation.is_some()
            && item.state == AppearanceState::Proposed
            && item.applied_at.is_none()
            && config.appearance_active.as_ref() != Some(&item.id)
            && proposal.previous_active.as_ref() != Some(&item.id)
            && !history
                .iter()
                .any(|entry| entry.previous_active.as_ref() == Some(&item.id))
    };
    let oldest = if let Some(sender) = &proposal.proposed_by_installation {
        let same_sender = |item: &AppearanceProposal| {
            item.proposed_by_installation.as_ref() == Some(sender)
                && item.state == AppearanceState::Proposed
        };
        let pending = history.iter().filter(|item| same_sender(item)).count();
        if pending >= REMOTE_PENDING_PER_SENDER || history.len() >= PROPOSAL_LIMIT {
            Some(history.iter().position(|item| same_sender(item) && disposable_remote(item))
                .ok_or_else(|| (StatusCode::CONFLICT, "Appearance proposal history is full. The owner can create the next proposal in Appearance Lab.".into()))?)
        } else {
            None
        }
    } else if history.len() >= PROPOSAL_LIMIT {
        history
            .iter()
            .position(disposable_remote)
            .or_else(|| {
                history
                    .iter()
                    .position(|item| proposal.previous_active.as_ref() != Some(&item.id))
            })
            .or(Some(0))
    } else {
        None
    };
    let history = &mut config.appearance_proposals;
    if let Some(oldest) = oldest {
        history.remove(oldest);
    }
    history.push(proposal);
    Ok(())
}

#[cfg(test)]
mod tests;
