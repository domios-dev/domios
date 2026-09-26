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
        let model = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_provider_resolved_model.clone());
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
        let (identity_changed, owned_actual) = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            let identity_changed =
                apply_session_owned_actual(tab, agent_id, session_id, actual.clone());
            let owned_actual = (tab.agent_type.as_deref() == Some(agent_id)
                && tab.agent_session_id.as_deref() == Some(session_id)
                && tab.reasoning_effort_identity.actual_source.as_deref() == Some(SESSION_OWNED))
                .then(|| tab.reasoning_effort_identity.actual.clone())
                .flatten();
            (identity_changed, owned_actual)
        };
        if identity_changed {
            self.emit_state_changed();
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
        let Some((plan_id, task_id, attempt_id)) = self
            .plan_store
            .open_attempt_task_for_effort_for_tab(
                tab_id,
                &binding.session_id,
                &binding.session_binding_nonce,
            )
        else {
            return;
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

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn attempt_records_actual_effort_for_claude_and_codex() {
        use crate::agent::test_provider::MockProvider;
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::{AssignmentRuntimeBinding, NewTask};

        let (mut mux, claude_tab) = crate::multiplexer::state::MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&claude_tab).unwrap();
        let codex_tab = mux.create_tab(&workspace_id).unwrap();
        let identities = [
            ("claude", "claude-model", "claude-session", "high"),
            ("codex", "codex-model", "codex-session", "xhigh"),
        ];
        for (tab_id, (provider, model, session, _)) in
            [&claude_tab, &codex_tab].into_iter().zip(identities)
        {
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.agent_type = Some(provider.into());
            tab.agent_session_id = Some(session.into());
            tab.session_binding_nonce = Some(format!("nonce-{session}"));
            tab.reported_session_binding_nonce = Some(format!("nonce-{session}"));
            tab.agent_provider_resolved_model = Some(model.into());
            tab.cwd = Some("/fixture/project".into());
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        for (provider, model, session, actual) in identities {
            fixture.service.agent_registry.register_provider(Box::new(
                MockProvider::with_model_reasoning_efforts(
                    provider,
                    model,
                    &["low", "medium", "high", "xhigh"],
                )
                .with_session_reasoning_effort(session, actual)
                .with_session_reasoning_effort("foreign-session", "medium"),
            ));
        }

        let mut plan = Plan::new(
            "Session effort identity".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&claude_tab),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        for (tab_id, (provider, model, session, actual)) in
            [&claude_tab, &codex_tab].into_iter().zip(identities)
        {
            let task_id = fixture
                .service
                .plan_store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: format!("{provider} task"),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap();
            let nonce = format!("nonce-{session}");
            fixture
                .service
                .plan_store
                .assign_task_with_runtime_binding(
                    &plan.id,
                    &task_id,
                    tab_id,
                    None,
                    None,
                    Some(&AssignmentRuntimeBinding {
                        session_id: session.into(),
                        session_binding_nonce: nonce,
                        provider: Some(provider.into()),
                        model: Some(model.into()),
                        session_model: Some(model.into()),
                        reasoning_effort: Some("medium".into()),
                    }),
                    None,
                    3,
                )
                .unwrap();
            fixture.service.refresh_session_reasoning_effort_once(
                tab_id,
                provider,
                "/fixture/project",
                session,
            );
            let task = fixture
                .service
                .plan_store
                .get(&plan.id)
                .unwrap()
                .tasks
                .into_iter()
                .find(|task| task.id == task_id)
                .unwrap();
            assert_eq!(
                task.execution_attempts[0]
                    .actual_reasoning_effort
                    .as_deref(),
                Some(actual)
            );
            assert_eq!(
                task.execution_attempts[0].reasoning_effort.as_deref(),
                Some("medium")
            );
        }
    }
}
