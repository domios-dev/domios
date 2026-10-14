use super::*;
use crate::service::agent_runtime::plan_warning::PlanBindingFailure;

impl ServiceContext {
    pub(crate) fn prepare_runtime_worker_binding(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let mut prepared = params.clone();
        if prepared.remote_task_binding.is_none()
            && prepared.plan_task.is_none()
            && prepared.task_launch_authority.is_some()
        {
            prepared.plan_task = prepared.task_id.clone();
        }
        self.bind_worker_plan_task_or_rollback_tab(&prepared, tab_id)
    }

    pub(crate) fn finalize_runtime_worker_session(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<(), String> {
        if let Err(reason) = self.try_finalize_runtime_worker_session(tab_id, thread, nonce) {
            self.warn_runtime_plan_binding(
                tab_id,
                crate::service::agent_runtime::plan_warning::PlanBindingWarningStage::Finalization,
                &reason,
            );
        }
        Ok(())
    }

    fn try_finalize_runtime_worker_session(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<(), PlanBindingFailure> {
        self.restore_runtime_attempt_binding(tab_id)?;
        self.bind_planner_session(tab_id);
        if self.finalize_authorized_runtime_restart_binding(tab_id, thread, nonce)? {
            return Ok(());
        }
        let (binding, restored, remote) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.task_binding.clone(),
                    tab.restored_from_snapshot,
                    tab.report_target_mesh_peer.is_some(),
                )
            })
            .unwrap_or_default();
        let Some(binding) = binding else {
            return Ok(());
        };
        let (Some(plan), Some(task), Some(attempt)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("Runtime task binding incomplete".into());
        };
        let Some(detail) = self.plan_store.show_task(&task) else {
            return if remote {
                Ok(())
            } else {
                Err(PlanBindingFailure::bookkeeping(
                    "Runtime task disappeared before readiness",
                ))
            };
        };
        if detail.plan_id != plan {
            return Err("Runtime task plan changed".into());
        }
        let existing = detail
            .task
            .execution_attempts
            .iter()
            .find(|entry| entry.id == attempt && entry.executor_tab_id == tab_id)
            .ok_or("Runtime task attempt changed")?;
        if needs_attempt_finalization(
            existing.session_id.as_deref(),
            existing.session_binding_nonce.as_deref(),
            existing.closed_at.is_some(),
            existing.retired_at.is_some(),
            restored,
            thread,
            nonce,
        )? {
            if restored {
                self.plan_store.bind_restored_runtime_task_session(
                    crate::service::plan::task_attempts::RestoredRuntimeSessionBinding {
                        plan_id: plan,
                        task_id: task,
                        attempt_id: attempt,
                        executor_tab_id: tab_id.into(),
                        thread: thread.into(),
                        nonce: nonce.into(),
                        expected_thread: existing.session_id.clone(),
                        expected_nonce: existing.session_binding_nonce.clone(),
                        owner_epoch: detail.task.interruption.owner_epoch,
                    },
                    epoch_ms(),
                )
            } else {
                self.plan_store.finish_task_dispatch(
                    &plan,
                    &task,
                    tab_id,
                    &attempt,
                    thread,
                    nonce,
                    detail.task.lane,
                    epoch_ms(),
                )
            }
            .map_err(PlanBindingFailure::from_plan_error)?;
        }
        Ok(())
    }
}

fn needs_attempt_finalization(
    saved_thread: Option<&str>,
    saved_nonce: Option<&str>,
    closed: bool,
    retired: bool,
    restored: bool,
    thread: &str,
    nonce: &str,
) -> Result<bool, String> {
    if retired {
        return Err("Runtime task attempt retired".into());
    }
    if saved_thread.is_some_and(|bound| bound != thread)
        || (!restored && saved_nonce.is_some_and(|bound| bound != nonce))
    {
        return Err("Runtime task session rebound".into());
    }
    if closed {
        return if saved_thread == Some(thread) {
            Ok(false)
        } else {
            Err("Closed runtime attempt has no bound conversation".into())
        };
    }
    Ok(saved_thread.is_none() || saved_nonce != Some(nonce))
}

#[cfg(test)]
mod tests {
    use super::super::tests::store_with_task;
    use super::*;
    #[test]
    fn completed_attempt_resumes_only_its_conversation_without_reopening_execution() {
        assert_eq!(
            needs_attempt_finalization(Some("own"), Some("old"), true, false, true, "own", "new"),
            Ok(false)
        );
        assert!(needs_attempt_finalization(
            Some("other"),
            Some("old"),
            true,
            false,
            true,
            "own",
            "new"
        )
        .is_err());
        assert!(
            needs_attempt_finalization(None, Some("old"), true, false, true, "own", "new").is_err()
        );
        assert!(needs_attempt_finalization(
            Some("own"),
            Some("old"),
            true,
            false,
            false,
            "own",
            "new"
        )
        .is_err());
        assert!(needs_attempt_finalization(
            Some("own"),
            Some("old"),
            true,
            true,
            true,
            "own",
            "new"
        )
        .is_err());
        assert_eq!(
            needs_attempt_finalization(Some("own"), Some("old"), false, false, true, "own", "new"),
            Ok(true)
        );
        assert_eq!(
            needs_attempt_finalization(
                Some("own"),
                Some("same"),
                false,
                false,
                false,
                "own",
                "same"
            ),
            Ok(false)
        );
    }
    #[test]
    fn prepared_task_attempt_is_committed_once_before_runtime_and_finalized_on_exact_session() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan, task) = store_with_task(dir.path());
        let bind = |session, nonce| {
            bind_worker_task_to_tab(
                &store,
                Some(&plan),
                "spawn-bind",
                "tab-worker",
                "Ada",
                session,
                nonce,
                crate::plan::ExecutionIntent::ReadOnly,
                None,
                Some("fixture-provider"),
                None,
                None,
                None,
                "attempt-prepared",
                None,
                Vec::new(),
                10,
            )
        };
        bind(None, None).unwrap();
        bind(None, None).unwrap();
        bind(Some("own-session"), Some("own-nonce")).unwrap();
        bind(Some("own-session"), Some("own-nonce")).unwrap();
        assert!(bind(Some("foreign-session"), Some("own-nonce")).is_err());
        let task = store.show_task(&task).unwrap().task;
        assert_eq!(task.execution_attempts.len(), 1);
        assert_eq!(task.execution_attempts[0].id, "attempt-prepared");
        assert_eq!(
            task.execution_attempts[0].session_id.as_deref(),
            Some("own-session")
        );
        assert_eq!(
            task.execution_attempts[0].session_binding_nonce.as_deref(),
            Some("own-nonce")
        );
    }
    #[test]
    fn runtime_routed_plan_dispatch_binds_intent_authority_manifest_and_accepts_worker_ack() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = mux.create_workspace("fixture".into(), None);
        let tab_id = mux.create_tab(&workspace.id).unwrap();
        mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("runtime-fixture".into());
        let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let (store, plan, task) = store_with_task(fixture.temp.path());
        fixture.service.plan_store = std::sync::Arc::new(store);
        let registry = crate::service::spawn_capability::test_backend::registry_with_runtime(
            "runtime-fixture",
        );
        let mut params = SpawnAgentParams {
            agent: Some("runtime-fixture".into()),
            role: Some("Worker".into()),
            plan_id: Some(plan.clone()),
            plan_task: Some(task.clone()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            authorized_gates: vec!["opaque-gate".into()],
            ..Default::default()
        };
        fixture
            .service
            .preflight_worker_plan_task_spawn(&params)
            .unwrap();
        fixture
            .service
            .prepare_worker_plan_task_spawn(&mut params)
            .unwrap();
        let manifest = vec![crate::plan::PlanSkillManifestEntry {
            slug: "opaque-skill".into(),
            title: "opaque-title".into(),
            purpose: "opaque-purpose".into(),
            content_digest: "sha256:opaque-digest".into(),
        }];
        params.plan_skill_manifest = Some(manifest.clone());
        let authority = params.task_launch_authority.clone().unwrap();
        assert!(registry
            .descriptor("runtime-fixture")
            .unwrap()
            .agent_runtime
            .is_some());
        let mut spec = crate::service::spawn::SpawnTabSpec::from_agent_params(params);
        spec.route_agent_to_chatbackend().unwrap();
        assert!(matches!(
            spec.command,
            crate::service::spawn::TabCommand::ChatBackend { .. }
        ));
        let metadata = spec.chatbackend_launch.as_ref().unwrap();
        fixture
            .service
            .prepare_runtime_worker_binding(&metadata.params, &tab_id)
            .unwrap();
        fixture
            .service
            .finalize_runtime_worker_session(&tab_id, "own-thread", "own-nonce")
            .unwrap();
        let detail = fixture.service.plan_store.show_task(&task).unwrap();
        assert_eq!(
            detail.task.status_label.as_deref(),
            Some("awaiting_worker_ack")
        );
        let attempt = detail.task.execution_attempts.last().unwrap();
        assert_eq!(attempt.id, authority.attempt_id);
        assert_eq!(attempt.provider.as_deref(), Some("runtime-fixture"));
        assert_eq!(
            attempt.authorized_gates.as_deref(),
            Some(&["opaque-gate".to_string()][..])
        );
        assert_eq!(
            detail.task.interruption.execution_intent,
            crate::plan::ExecutionIntent::ReadOnly
        );
        assert_eq!(
            serde_json::to_vec(&attempt.plan_skill_manifest).unwrap(),
            serde_json::to_vec(&manifest).unwrap()
        );
        assert_eq!(attempt.session_id.as_deref(), Some("own-thread"));
        assert_eq!(attempt.session_binding_nonce.as_deref(), Some("own-nonce"));
        assert_eq!(
            metadata.params.task_launch_authority.as_ref(),
            Some(&authority)
        );
        let binding = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .task_binding
            .clone()
            .unwrap();
        assert_eq!(binding.plan_id.as_deref(), Some(plan.as_str()));
        assert_eq!(binding.task_id.as_deref(), Some(task.as_str()));
        fixture
            .service
            .plan_store
            .acknowledge_task_attempt(&plan, &task, &tab_id, &authority.attempt_id, epoch_ms())
            .unwrap();
        assert_eq!(
            fixture
                .service
                .plan_store
                .show_task(&task)
                .unwrap()
                .task
                .status,
            crate::plan::TaskStatus::InProgress
        );
    }
    #[test]
    fn restored_worker_finalizes_plan_task_only_binding_onto_acknowledged_open_attempt() {
        assert_restored_worker_preserves_status(crate::plan::TaskStatus::InProgress);
    }

    #[test]
    fn restored_worker_in_review_binds_session_without_changing_review_state() {
        assert_restored_worker_preserves_status(crate::plan::TaskStatus::InReview);
    }

    #[test]
    fn restored_worker_preserves_non_progress_task_states() {
        for status in [
            crate::plan::TaskStatus::Pending,
            crate::plan::TaskStatus::Blocked,
            crate::plan::TaskStatus::Paused,
            crate::plan::TaskStatus::Done,
            crate::plan::TaskStatus::Cancelled,
        ] {
            assert_restored_worker_preserves_status(status);
        }
    }

    fn assert_restored_worker_preserves_status(status: crate::plan::TaskStatus) {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = mux.create_workspace("fixture".into(), None);
        let tab_id = mux.create_tab(&workspace.id).unwrap();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("runtime-fixture".into());
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
        tab.agent_session_id = Some("own-thread".into());
        tab.session_binding_nonce = Some("new-nonce".into());
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "runtime-fixture".into(),
            session_id: "own-thread".into(),
        };
        let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let (store, plan, task) = store_with_task(fixture.temp.path());
        fixture.service.plan_store = std::sync::Arc::new(store);
        let mut params = SpawnAgentParams {
            agent: Some("runtime-fixture".into()),
            role: Some("Worker".into()),
            plan_id: Some(plan.clone()),
            plan_task: Some(task.clone()),
            execution_intent: Some(crate::plan::ExecutionIntent::ReadOnly),
            ..Default::default()
        };
        fixture
            .service
            .prepare_worker_plan_task_spawn(&mut params)
            .unwrap();
        fixture
            .service
            .prepare_runtime_worker_binding(&params, &tab_id)
            .unwrap();
        let attempt_id = params
            .task_launch_authority
            .as_ref()
            .unwrap()
            .attempt_id
            .clone();
        fixture
            .service
            .plan_store
            .finish_task_dispatch(
                &plan,
                &task,
                &tab_id,
                &attempt_id,
                "own-thread",
                "old-nonce",
                fixture
                    .service
                    .plan_store
                    .show_task(&task)
                    .unwrap()
                    .task
                    .lane,
                10,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .acknowledge_task_attempt(&plan, &task, &tab_id, &attempt_id, 11)
            .unwrap();
        if status == crate::plan::TaskStatus::InReview {
            fixture
                .service
                .plan_store
                .record_task_attempt_report(
                    &plan,
                    &task,
                    &crate::plan::AgentRef::from_tab(&tab_id),
                    "opaque-report",
                    Some(codeterm_types::agent::ReportStatus::InReview),
                    12,
                )
                .unwrap();
        }
        fixture
            .service
            .plan_store
            .with_plan(&plan, 12, |plan| {
                let entry = plan
                    .tasks
                    .iter_mut()
                    .find(|entry| entry.id == task)
                    .unwrap();
                entry.status = status.clone();
                entry.status_label = Some("opaque-status".into());
                entry.status_changed_at = 12;
                entry.review_round = 3;
                Ok(())
            })
            .unwrap();
        let before = fixture.service.plan_store.show_task(&task).unwrap().task;
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .task_binding
            .as_mut()
            .unwrap()
            .attempt_id = None;
        let snapshot = fixture.service.mux.read().to_session_snapshot();
        *fixture.service.mux.write() =
            crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);
        fixture
            .service
            .finalize_runtime_worker_session(&tab_id, "own-thread", "new-nonce")
            .unwrap();
        let binding = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .task_binding
            .clone()
            .unwrap();
        assert_eq!(binding.attempt_id.as_deref(), Some(attempt_id.as_str()));
        let detail = fixture.service.plan_store.show_task(&task).unwrap();
        assert_eq!(detail.task.execution_attempts.len(), 1);
        let attempt = &detail.task.execution_attempts[0];
        assert_eq!(attempt.id, attempt_id);
        assert_eq!(attempt.session_id.as_deref(), Some("own-thread"));
        assert_eq!(attempt.session_binding_nonce.as_deref(), Some("new-nonce"));
        assert_eq!(attempt.acknowledged_at, Some(11));
        assert_eq!(detail.task.status, status);
        assert_eq!(detail.task.status_label, before.status_label);
        assert_eq!(detail.task.status_changed_at, before.status_changed_at);
        assert_eq!(detail.task.review_round, before.review_round);
        assert_eq!(
            serde_json::to_value(&detail.task.review_evidence).unwrap(),
            serde_json::to_value(&before.review_evidence).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&detail.task.lane).unwrap(),
            serde_json::to_value(&before.lane).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&attempt.report).unwrap(),
            serde_json::to_value(&before.execution_attempts[0].report).unwrap()
        );
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .task_binding
            .as_mut()
            .unwrap()
            .attempt_id = None;
        assert!(fixture
            .service
            .authorize_runtime_restart_binding(&tab_id, "runtime-fixture", "new-nonce", false,)
            .unwrap()
            .is_some());
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .task_binding
                .as_ref()
                .unwrap()
                .attempt_id
                .as_deref(),
            Some(attempt_id.as_str())
        );
    }

    #[test]
    fn prepared_dispatch_brief_is_not_rewritten_when_gates_are_carried_to_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut params = SpawnAgentParams {
            task: Some("opaque-prepared-brief".into()),
            authorized_gates: vec!["opaque-gate".into()],
            task_launch_authority: Some(crate::service::TaskLaunchAuthority {
                plan_id: "opaque-plan".into(),
                task_id: "opaque-task".into(),
                attempt_id: "opaque-attempt".into(),
                lane_base: None,
            }),
            ..Default::default()
        };
        prepare_worker_plan_task_prompt(&store, &mut params).unwrap();
        assert_eq!(params.task.as_deref(), Some("opaque-prepared-brief"));
        assert_eq!(params.authorized_gates, ["opaque-gate"]);
    }
}
