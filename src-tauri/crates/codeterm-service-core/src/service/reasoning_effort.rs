use super::ServiceContext;
use crate::multiplexer::types::{ReasoningEffortIdentity, Tab};
use crate::service::MuxEmitCause;

const UNVERIFIED: &str = "provider-default-unverified";
const SESSION_OWNED: &str = "session-owned";

struct EffortObservation {
    identity: ReasoningEffortIdentity,
    nonce: Option<String>,
    acknowledged_nonce: Option<String>,
}

impl EffortObservation {
    fn capture(tab: &Tab) -> Self {
        Self {
            identity: tab.reasoning_effort_identity.clone(),
            nonce: tab.session_binding_nonce.clone(),
            acknowledged_nonce: tab.reported_session_binding_nonce.clone(),
        }
    }

    fn current(&self, tab: &Tab) -> bool {
        self.identity == tab.reasoning_effort_identity
            && self.nonce == tab.session_binding_nonce
            && self.acknowledged_nonce == tab.reported_session_binding_nonce
    }
}

fn apply_observed_actual(
    tab: &mut Tab,
    agent_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    actual: Option<String>,
    expected: Option<&EffortObservation>,
) -> bool {
    expected.is_none_or(|expected| expected.current(tab))
        && apply_session_owned_actual(tab, agent_id, session_id, binding_nonce, actual)
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn persist_launch_identity(
    tab: &mut Tab,
    planned: Option<&str>,
    requested: Option<&str>,
) {
    let requested = normalized(requested);
    let planned = normalized(planned);
    let has_effort = planned.is_some() || requested.is_some();
    tab.reasoning_effort_identity = ReasoningEffortIdentity {
        planned,
        requested: requested.clone(),
        resolved: requested,
        actual: None,
        actual_source: has_effort.then(|| UNVERIFIED.to_string()),
    };
}

pub(crate) fn persist_completed_launch_identity(
    tab: &mut Tab,
    planned: Option<&str>,
    requested: Option<&str>,
) {
    let authenticated = tab
        .agent_session_id
        .as_ref()
        .is_some_and(|id| !id.is_empty())
        && tab
            .session_binding_nonce
            .as_deref()
            .is_some_and(|nonce| tab.reported_session_binding_nonce.as_deref() == Some(nonce))
        && tab.reasoning_effort_identity.actual_source.as_deref() == Some(SESSION_OWNED);
    let actual = authenticated
        .then(|| tab.reasoning_effort_identity.actual.clone())
        .flatten();
    persist_launch_identity(tab, planned, requested);
    if let Some(actual) = actual {
        tab.reasoning_effort_identity.actual = Some(actual);
        tab.reasoning_effort_identity.actual_source = Some(SESSION_OWNED.into());
    }
}

#[cfg(test)]
#[path = "reasoning_effort/completed_launch_tests.rs"]
mod completed_launch_tests;

pub(crate) fn clear_runtime_evidence(tab: &mut Tab) {
    tab.reasoning_effort_identity.actual = None;
    if !tab.reasoning_effort_identity.is_empty() {
        tab.reasoning_effort_identity.actual_source = Some(UNVERIFIED.to_string());
    }
}

pub(crate) fn apply_session_owned_actual(
    tab: &mut Tab,
    agent_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    actual: Option<String>,
) -> bool {
    if tab.agent_type.as_deref() != Some(agent_id)
        || tab.agent_session_id.as_deref() != Some(session_id)
        || binding_nonce.is_some_and(|nonce| {
            tab.session_binding_nonce.as_deref() != Some(nonce)
                || tab.reported_session_binding_nonce.as_deref() != Some(nonce)
        })
        || (actual.is_none() && tab.reasoning_effort_identity.is_empty())
    {
        return false;
    }
    let source = if actual.is_some() {
        SESSION_OWNED
    } else {
        UNVERIFIED
    };
    if tab.reasoning_effort_identity.actual == actual
        && tab.reasoning_effort_identity.actual_source.as_deref() == Some(source)
    {
        return false;
    }
    tab.reasoning_effort_identity.actual = actual;
    tab.reasoning_effort_identity.actual_source = Some(source.to_string());
    true
}

impl ServiceContext {
    pub(crate) fn refresh_session_reasoning_effort_once(
        &self,
        tab_id: &str,
        agent_id: &str,
        cwd: &str,
        session_id: &str,
    ) {
        let observation = self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.agent_provider_resolved_model.clone(),
                EffortObservation::capture(tab),
            )
        });
        let Some((model, observation)) = observation else {
            return;
        };
        let actual = self
            .agent_registry
            .with_provider(agent_id, |provider| {
                provider
                    .detect_session_reasoning_effort(cwd, session_id)
                    .filter(|effort| {
                        provider.supports_reasoning_effort_for_model(model.as_deref(), effort)
                    })
            })
            .flatten();
        self.record_observed_reasoning_effort(
            tab_id,
            agent_id,
            session_id,
            observation.nonce.as_deref(),
            actual,
            Some(&observation),
        );
    }

    pub(crate) fn record_session_reasoning_effort(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        binding_nonce: Option<&str>,
        actual: Option<String>,
    ) {
        self.record_observed_reasoning_effort(
            tab_id,
            agent_id,
            session_id,
            binding_nonce,
            actual,
            None,
        );
    }

    fn record_observed_reasoning_effort(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        binding_nonce: Option<&str>,
        actual: Option<String>,
        expected: Option<&EffortObservation>,
    ) {
        let (identity_changed, owned_actual) = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            let identity_changed =
                apply_observed_actual(tab, agent_id, session_id, binding_nonce, actual, expected);
            let owned_actual = (tab.agent_type.as_deref() == Some(agent_id)
                && tab.agent_session_id.as_deref() == Some(session_id)
                && tab.reasoning_effort_identity.actual_source.as_deref() == Some(SESSION_OWNED))
            .then(|| tab.reasoning_effort_identity.actual.clone())
            .flatten();
            (identity_changed, owned_actual)
        };
        if identity_changed {
            self.emit_state_changed(MuxEmitCause::Model);
            if let Err(error) = super::session_persist::persist_session_after_mutation(
                self,
                super::session_persist::SessionPersistUrgency::Prompt,
            ) {
                log::warn!("[reasoning-effort] failed to persist session attestation: {error}");
            }
        }
        if let Some(actual) = owned_actual {
            self.persist_attempt_actual_reasoning_effort(tab_id, agent_id, session_id, &actual);
        }
    }

    fn persist_attempt_actual_reasoning_effort(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        actual: &str,
    ) {
        let binding = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if tab.agent_type.as_deref() != Some(agent_id)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.reasoning_effort_identity.actual.as_deref() != Some(actual)
                || tab.reasoning_effort_identity.actual_source.as_deref() != Some(SESSION_OWNED)
            {
                return;
            }
            super::task_assignment::authenticated_tab_binding(tab)
        };
        let Some(binding) = binding else { return };
        if binding.session_id != session_id {
            return;
        }
        let (plan_id, task_id, attempt_id) =
            match self.plan_store.open_attempt_task_for_effort_for_tab(
                tab_id,
                &binding.session_id,
                &binding.session_binding_nonce,
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) => return,
                Err(error) => {
                    log::error!(
                        "[reasoning-effort] task binding projection failed tab={tab_id}: {error}"
                    );
                    return;
                }
            };
        match self.plan_store.update_dispatched_attempt_actual_reasoning_effort(
            &plan_id,
            &task_id,
            tab_id,
            &attempt_id,
            &binding.session_id,
            &binding.session_binding_nonce,
            actual.to_string(),
            session_effort_epoch_ms(),
        ) {
            Ok(true) => self.emit_plan_changed(&plan_id),
            Ok(false) => {}
            Err(error) => log::warn!(
                "[reasoning-effort] failed to persist task attempt actual effort tab={tab_id}: {error}"
            ),
        }
    }
}

fn session_effort_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
