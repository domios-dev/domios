use crate::multiplexer::commands::MuxState;
use crate::service::plan::{PlanError, PlanStore};

use super::{ServiceState, SpawnIdentityProvenance};

pub(crate) fn respawn_title_provenance(explicit_title: bool) -> SpawnIdentityProvenance {
    if explicit_title {
        SpawnIdentityProvenance::UserSupplied
    } else {
        SpawnIdentityProvenance::Suggested
    }
}

pub(crate) fn persist_attempt_executor_identity(
    store: &PlanStore,
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    let result = persist_attempt_executor_identity_for_mux(
        store,
        &svc.mux,
        plan_id,
        task_id,
        tab_id,
        title,
        now,
    );
    if result.is_ok() {
        svc.persist_current_attempt_session_model(tab_id);
    }
    result
}

pub(crate) fn persist_attempt_executor_identity_for_mux(
    store: &PlanStore,
    mux: &MuxState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    let mux = mux.read();
    let binding = mux
        .get_tab(tab_id)
        .and_then(super::task_assignment::authenticated_tab_binding);
    store.update_attempt_executor_identity(
        plan_id,
        task_id,
        tab_id,
        Some(title),
        binding.as_ref().map(|binding| binding.session_id.as_str()),
        binding
            .as_ref()
            .map(|binding| binding.session_binding_nonce.as_str()),
        now,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::TimedRwLock;
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
    use crate::service::plan::NewTask;
    use std::sync::Arc;

    #[test]
    fn respawn_title_provenance_preserves_explicit_user_titles() {
        assert_eq!(
            respawn_title_provenance(true),
            SpawnIdentityProvenance::UserSupplied
        );
        assert_eq!(
            respawn_title_provenance(false),
            SpawnIdentityProvenance::Suggested
        );
    }

    #[test]
    fn lifecycle_reassignment_respawn_identity_requires_verified_binding() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let (state, _) = MultiplexerState::new();
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", state));
        let mut plan = Plan::new(
            "Respawn".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "task".into(),
                    kind: "code".into(),
                    lane: TaskLaneMetadata {
                        workspace_id: Some("ws".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let draft = crate::service::plan::task_attempts::TaskRespawnDraft {
            plan_id: plan.id.clone(),
            task_id: task_id.clone(),
            source_attempt: Default::default(),
            provider: "claude".into(),
            planned_model: None,
            model: None,
            reasoning_effort: None,
            parent_tab_id: None,
            workspace_id: Some("ws".into()),
            title: Some("user-title".into()),
            explicit_title: true,
            cwd: ".".into(),
            task_prompt: "prompt".into(),
            report_command: "report".into(),
            plan_skill_manifest: vec![],
            authorized_gates: None,
        };
        let ws = mux
            .write()
            .create_workspace("Respawn".into(), Some(dir.path().display().to_string()))
            .id;
        let tab_id = mux.write().create_tab(&ws).unwrap();
        mux.write().get_tab_mut(&tab_id).unwrap().agent_session_id = Some("sess-1".into());
        mux.write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some("test-nonce".into());
        store
            .record_task_respawn_attempt(&plan.id, &task_id, &tab_id, &draft, Some("worker"), 3)
            .unwrap();
        for reported in [None, Some("other-nonce")] {
            mux.write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .reported_session_binding_nonce = reported.map(str::to_string);
            persist_attempt_executor_identity_for_mux(
                &store, &mux, &plan.id, &task_id, &tab_id, "Ada", 4,
            )
            .unwrap();
            let task = store.show_task(&task_id).unwrap().task;
            let attempt = task.execution_attempts.last().unwrap();
            assert!(attempt.session_id.is_none());
            assert!(attempt.session_binding_nonce.is_none());
            assert!(attempt.acknowledged_at.is_none());
            assert_eq!(task.status_label.as_deref(), Some("starting"));
        }
        mux.write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .reported_session_binding_nonce = Some("test-nonce".into());
        persist_attempt_executor_identity_for_mux(
            &store, &mux, &plan.id, &task_id, &tab_id, "Ada", 5,
        )
        .unwrap();
        let task = store.show_task(&task_id).unwrap();
        let attempt = task
            .task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.executor_tab_id == tab_id)
            .unwrap();
        assert_eq!(attempt.executor_title.as_deref(), Some("Ada"));
        assert_eq!(attempt.session_id.as_deref(), Some("sess-1"));
        assert_eq!(attempt.session_binding_nonce.as_deref(), Some("test-nonce"));
        assert!(attempt.acknowledged_at.is_none());
        assert_eq!(task.task.status_label.as_deref(), Some("starting"));
        store
            .finish_task_dispatch(
                &plan.id,
                &task_id,
                &tab_id,
                &attempt.id,
                "sess-1",
                "test-nonce",
                task.task.lane.clone(),
                6,
            )
            .unwrap();
        assert_eq!(
            store
                .show_task(&task_id)
                .unwrap()
                .task
                .status_label
                .as_deref(),
            Some("awaiting_worker_ack")
        );
    }
}
