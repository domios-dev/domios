use super::ServiceContext;
use crate::multiplexer::types::{ReasoningEffortIdentity, Tab};

const UNVERIFIED: &str = "provider-default-unverified";
const SESSION_OWNED: &str = "session-owned";

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

pub(crate) fn clear_runtime_evidence(tab: &mut Tab) {
    tab.reasoning_effort_identity.actual = None;
    if !tab.reasoning_effort_identity.is_empty() {
        tab.reasoning_effort_identity.actual_source = Some(UNVERIFIED.to_string());
    }
}

fn apply_session_owned_actual(
    tab: &mut Tab,
    agent_id: &str,
    session_id: &str,
    actual: Option<String>,
) -> bool {
    if tab.agent_type.as_deref() != Some(agent_id)
        || tab.agent_session_id.as_deref() != Some(session_id)
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
        let actual = self
            .agent_registry
            .with_provider(agent_id, |provider| {
                provider
                    .detect_session_reasoning_effort(cwd, session_id)
                    .filter(|effort| provider.supports_reasoning_effort(effort))
            })
            .flatten();
        let mut mux = self.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if !apply_session_owned_actual(tab, agent_id, session_id, actual) {
            return;
        }
        drop(mux);
        self.emit_state_changed();
        if let Err(error) = super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        ) {
            log::warn!("[reasoning-effort] failed to persist session attestation: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_owned_effort_attestation_never_uses_global_latest() {
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.agent_session_id = Some("owned-session".into());
        persist_launch_identity(tab, Some("high"), Some("medium"));

        assert_eq!(
            tab.reasoning_effort_identity.planned.as_deref(),
            Some("high")
        );
        assert_eq!(
            tab.reasoning_effort_identity.requested.as_deref(),
            Some("medium")
        );
        assert_eq!(
            tab.reasoning_effort_identity.resolved.as_deref(),
            Some("medium")
        );

        assert!(!apply_session_owned_actual(
            tab,
            "claude",
            "latest-foreign-session",
            Some("max".into()),
        ));
        assert_eq!(tab.reasoning_effort_identity.actual, None);
        assert_eq!(
            tab.reasoning_effort_identity.actual_source.as_deref(),
            Some("provider-default-unverified")
        );
        assert!(apply_session_owned_actual(
            tab,
            "claude",
            "owned-session",
            Some("high".into()),
        ));
        assert_eq!(
            tab.reasoning_effort_identity.actual.as_deref(),
            Some("high")
        );
        assert_eq!(
            tab.reasoning_effort_identity.actual_source.as_deref(),
            Some("session-owned")
        );
    }

    #[test]
    fn non_plan_launch_keeps_planned_effort_null() {
        let (mut state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        persist_launch_identity(tab, None, Some("high"));

        assert_eq!(tab.reasoning_effort_identity.planned, None);
        assert_eq!(
            tab.reasoning_effort_identity.requested.as_deref(),
            Some("high")
        );
        assert_eq!(
            tab.reasoning_effort_identity.resolved.as_deref(),
            Some("high")
        );
    }
}
