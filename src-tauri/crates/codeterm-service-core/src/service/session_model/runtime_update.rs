use super::*;
use crate::multiplexer::{
    commands::MuxState,
    types::{
        LaunchModelReport, ModelFallbackNotice, ModelIdentityMismatch, SessionModelIdentity, Tab,
    },
};

#[derive(Clone, PartialEq, Eq)]
struct ModelValues {
    model: Option<String>,
    resolved: Option<String>,
    identity: Option<SessionModelIdentity>,
    fallback: Option<ModelFallbackNotice>,
    mismatch: Option<ModelIdentityMismatch>,
}

#[derive(Clone, PartialEq, Eq)]
struct ModelSnapshot {
    provider: Option<String>,
    session: Option<String>,
    nonce: Option<String>,
    reported_nonce: Option<String>,
    planned: Option<String>,
    requested: Option<String>,
    values: ModelValues,
}

impl ModelSnapshot {
    fn capture(tab: &Tab) -> Self {
        Self {
            provider: tab.agent_type.clone(),
            session: tab.agent_session_id.clone(),
            nonce: tab.session_binding_nonce.clone(),
            reported_nonce: tab.reported_session_binding_nonce.clone(),
            planned: tab.agent_planned_model.clone(),
            requested: tab.agent_requested_model.clone(),
            values: ModelValues {
                model: tab.agent_session_model.clone(),
                resolved: tab.agent_provider_resolved_model.clone(),
                identity: tab.agent_session_model_identity.clone(),
                fallback: tab.model_fallback.clone(),
                mismatch: tab.model_identity_mismatch.clone(),
            },
        }
    }

    fn matches(&self, tab: &Tab) -> bool {
        self.provider == tab.agent_type
            && self.session == tab.agent_session_id
            && self.nonce == tab.session_binding_nonce
            && self.reported_nonce == tab.reported_session_binding_nonce
            && self.planned == tab.agent_planned_model
            && self.requested == tab.agent_requested_model
            && self.values.model == tab.agent_session_model
            && self.values.resolved == tab.agent_provider_resolved_model
            && self.values.identity == tab.agent_session_model_identity
            && self.values.fallback == tab.model_fallback
            && self.values.mismatch == tab.model_identity_mismatch
    }

    fn verified(&self, session: &str) -> bool {
        self.session.as_deref() == Some(session)
            && self.nonce.is_some()
            && self.reported_nonce == self.nonce
            && self.values.identity.as_ref().is_some_and(|identity| {
                identity.session_id == session
                    && Some(&identity.session_binding_nonce) == self.nonce.as_ref()
            })
    }
}

pub(super) struct ModelUpdate {
    pub(super) model_changed: bool,
    pub(super) previous_model: Option<String>,
    pub(super) state_changed: bool,
    pub(super) should_notify: bool,
    pub(super) mismatch: Option<ModelIdentityMismatch>,
}

fn compute(
    snapshot: &ModelSnapshot,
    session: &str,
    model: &str,
    launch_report: Option<LaunchModelReport>,
    post_relaunch_turn: bool,
    requested_matches: bool,
) -> (ModelValues, ModelUpdate) {
    let identity_update = model_identity_update(
        snapshot.values.model.as_deref(),
        snapshot.planned.as_deref(),
        snapshot.requested.as_deref(),
        snapshot.values.resolved.as_deref(),
        snapshot.values.fallback.clone(),
        model,
        (snapshot.values.model.is_some()
            && snapshot.verified(session)
            && snapshot.values.mismatch.is_none()
            && snapshot.values.fallback.is_none())
            || requested_matches,
    );
    let state_changed = identity_update.model_changed
        || identity_update.mismatch != snapshot.values.mismatch
        || identity_update.fallback != snapshot.values.fallback;
    let mut values = snapshot.values.clone();
    values.identity = bound_identity(
        snapshot.session.as_deref(),
        snapshot.nonce.as_deref(),
        snapshot.reported_nonce.as_deref(),
        snapshot.values.identity.as_ref(),
        session,
        launch_report,
        post_relaunch_turn,
    );
    let update = ModelUpdate {
        model_changed: identity_update.model_changed,
        previous_model: snapshot.values.model.clone(),
        state_changed,
        should_notify: state_changed
            && runtime_mismatch_notice_needed(
                snapshot.values.fallback.as_ref(),
                identity_update.mismatch.as_ref(),
            ),
        mismatch: identity_update.mismatch.clone(),
    };
    if state_changed {
        values.model = Some(model.into());
        values.resolved = Some(model.into());
        values.fallback = identity_update.fallback;
        values.mismatch = identity_update.mismatch;
    }
    (values, update)
}

pub(super) fn update(
    mux: &MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider: &str,
    session: &str,
    model: &str,
    launch_report: Option<LaunchModelReport>,
    nonce: Option<&str>,
    post_relaunch_turn: bool,
) -> Option<ModelUpdate> {
    for _ in 0..2 {
        let snapshot = {
            let state = mux.read();
            let tab = state.get_tab(tab_id)?;
            if !runtime_model_update_matches_binding(tab, provider, session, nonce) {
                return None;
            }
            ModelSnapshot::capture(tab)
        };
        let verified_model = snapshot.values.model.is_some()
            && snapshot.verified(session)
            && snapshot.values.mismatch.is_none()
            && snapshot.values.fallback.is_none();
        let requested_matches = verified_model
            || snapshot.requested.as_deref().is_none_or(|requested| {
                session_model_matches(registry, provider, requested, model)
            });
        let (mut values, update) = compute(
            &snapshot,
            session,
            model,
            launch_report.clone(),
            post_relaunch_turn,
            requested_matches,
        );
        if values == snapshot.values {
            return Some(update);
        }
        let committed = {
            let mut state = mux.write();
            let tab = state.get_tab_mut(tab_id)?;
            if snapshot.matches(tab) {
                values.swap_into(tab);
                true
            } else {
                false
            }
        };
        if committed {
            return Some(update);
        }
    }
    log::debug!("[session-model] discarded contended update tab={tab_id}");
    None
}

impl ModelValues {
    fn swap_into(&mut self, tab: &mut Tab) {
        std::mem::swap(&mut tab.agent_session_model, &mut self.model);
        std::mem::swap(&mut tab.agent_provider_resolved_model, &mut self.resolved);
        std::mem::swap(&mut tab.agent_session_model_identity, &mut self.identity);
        std::mem::swap(&mut tab.model_fallback, &mut self.fallback);
        std::mem::swap(&mut tab.model_identity_mismatch, &mut self.mismatch);
    }
}

pub(super) fn bound_identity(
    bound_session: Option<&str>,
    nonce: Option<&str>,
    reported_nonce: Option<&str>,
    previous: Option<&SessionModelIdentity>,
    session: &str,
    launch_report: Option<LaunchModelReport>,
    post_relaunch_turn: bool,
) -> Option<SessionModelIdentity> {
    let nonce = nonce?;
    let already_confirmed = previous.is_some_and(|identity| {
        identity.session_id == session
            && identity.session_binding_nonce == nonce
            && identity.model_switch_turn_confirmed
    });
    (bound_session == Some(session) && reported_nonce == Some(nonce)).then(|| {
        SessionModelIdentity {
            session_id: session.into(),
            session_binding_nonce: nonce.into(),
            launch_report,
            model_switch_turn_confirmed: post_relaunch_turn || already_confirmed,
        }
    })
}

#[cfg(test)]
mod tests;
