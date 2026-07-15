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
    pane_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    persist_attempt_executor_identity_for_mux(
        store, &svc.mux, plan_id, task_id, pane_id, title, now,
    )
}

pub(crate) fn persist_attempt_executor_identity_for_mux(
    store: &PlanStore,
    mux: &MuxState,
    plan_id: &str,
    task_id: &str,
    pane_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    let session_id = mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.agent_session_id.clone());
    store.update_attempt_executor_identity(
        plan_id,
        task_id,
        pane_id,
        Some(title),
        session_id.as_deref(),
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
    fn persist_attempt_executor_identity_updates_title_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let (state, _) = MultiplexerState::new();
        let mux: MuxState = Arc::new(TimedRwLock::new("mux", state));
        let mut plan = Plan::new(
            "Respawn".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
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
            parent_pane_id: None,
            workspace_id: Some("ws".into()),
            title: Some("user-title".into()),
            explicit_title: true,
            cwd: ".".into(),
            task_prompt: "prompt".into(),
            report_command: "report".into(),
        };
        let ws = mux
            .write()
            .create_workspace("Respawn".into(), Some(dir.path().display().to_string()))
            .id;
        let (_tab_id, pane_id) = mux.write().create_tab(&ws).unwrap();
        mux.write().get_pane_mut(&pane_id).unwrap().agent_session_id = Some("sess-1".into());
        store
            .record_task_respawn_attempt(&plan.id, &task_id, &pane_id, &draft, Some("worker"), 3)
            .unwrap();
        persist_attempt_executor_identity_for_mux(
            &store, &mux, &plan.id, &task_id, &pane_id, "Ada", 4,
        )
        .unwrap();
        let task = store.show_task(&task_id).unwrap();
        let attempt = task
            .task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.executor_pane_id == pane_id)
            .unwrap();
        assert_eq!(attempt.executor_title.as_deref(), Some("Ada"));
        assert_eq!(attempt.session_id.as_deref(), Some("sess-1"));
    }
}
