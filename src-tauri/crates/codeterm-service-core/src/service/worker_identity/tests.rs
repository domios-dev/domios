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
    store
        .assign_task(&plan.id, &task_id, "source-tab", None, 2)
        .unwrap();
    let source_attempt = store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .execution_attempts[0]
        .clone();
    let draft = crate::service::plan::task_attempts::TaskRespawnDraft {
        launch_profile: None,
        plan_id: plan.id.clone(),
        task_id: task_id.clone(),
        execution_intent: crate::plan::ExecutionIntent::Mutating,
        attempt_id: "attempt-respawn".into(),
        source_attempt,
        provider: "claude".into(),
        planned_model: None,
        model: None,
        speed: None,
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
        .agent_runtime_config = Some(serde_json::json!({"launchProfile": "selected"}));
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
        let task = store.inspect_task_for_test(&task_id).unwrap().task;
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
    persist_attempt_executor_identity_for_mux(&store, &mux, &plan.id, &task_id, &tab_id, "Ada", 5)
        .unwrap();
    let task = store.inspect_task_for_test(&task_id).unwrap();
    let attempt = task
        .task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.executor_tab_id == tab_id)
        .unwrap();
    assert_eq!(attempt.launch_profile.as_deref(), Some("selected"));
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
            .inspect_task_for_test(&task_id)
            .unwrap()
            .task
            .status_label
            .as_deref(),
        Some("awaiting_worker_ack")
    );
}
