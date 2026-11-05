use super::*;
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::plan::{
    InterruptionCheckpoint, OrchestratorRef, Plan, TaskLaneMetadata,
};
use codeterm_service_core::service::plan::interruption::checkpoint_digest;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn checkpoint_adoption_keeps_mux_and_rest_responsive_and_refuses_changed_launch() {
    for change_launch in [false, true] {
        let (mut mux, owner) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&owner).unwrap();
        let worker = mux.create_tab(&workspace).unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&owner))
            .unwrap();
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.agent_type = Some("opaque-adopter".into());
        tab.agent_session_id = Some("new-session".into());
        tab.session_binding_nonce = Some("new-nonce".into());
        tab.reported_session_binding_nonce = Some("new-nonce".into());
        let fixture = ServiceFixture::new(mux);
        let state = app_state(&fixture);
        let store = state.svc.plan_store.clone();
        let mut plan = Plan::new(
            "Checkpoint liveness".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&owner),
        );
        store.save(&mut plan, 1).unwrap();
        let task = store
            .add_task(
                &plan.id,
                codeterm_service_core::service::plan::NewTask {
                    name: "Adopt exactly".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let attempt = store
            .begin_task_dispatch(
                &plan.id,
                &task,
                "old-tab",
                "Worker",
                TaskLaneMetadata::default(),
                3,
            )
            .unwrap();
        store
            .finish_task_dispatch(
                &plan.id,
                &task,
                "old-tab",
                &attempt,
                "old-session",
                "old-nonce",
                TaskLaneMetadata::default(),
                4,
            )
            .unwrap();
        store
            .acknowledge_live_task_attempt_for_tab("old-tab", Some(&plan.id), Some(&task), 4)
            .unwrap();
        let diff_artifact = "diff --git a/file b/file\n+dirty\n".to_string();
        let checkpoint = InterruptionCheckpoint {
            task_id: task.clone(),
            attempt_id: attempt,
            tab_id: "old-tab".into(),
            session_id: "old-session".into(),
            owner_epoch: 1,
            repo_path: "repo".into(),
            worktree_path: "worktree".into(),
            base_head: "base".into(),
            current_head: "head".into(),
            git_status: "1 .M file".into(),
            diff_sha256: checkpoint_digest(&diff_artifact),
            diff_artifact,
            completed_tests: vec![],
            unresolved_work: "finish".into(),
            next_acceptance_gate: "QA".into(),
        };
        store
            .interrupt_task_attempt(
                &plan.id,
                &task,
                &checkpoint.attempt_id,
                "old-tab",
                "old-session",
                1,
                "process_exited",
                checkpoint.clone(),
                5,
            )
            .unwrap();
        let before = store
            .get(&plan.id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|item| item.id == task)
            .unwrap();
        let held_store = store.clone();
        let held = HeldDependency::new(move |entered, release| {
            held_store.with_mutation_writer_held_for_test(|| {
                entered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(10)).unwrap();
            });
        })
        .await;
        let service = state.svc.clone();
        let (plan_id, task_id, tab_id) = (plan.id.clone(), task.clone(), worker.clone());
        let operation = tokio::task::spawn_blocking(move || {
            service.persist_checkpoint_adoption_for_test(
                &plan_id,
                &task_id,
                &tab_id,
                "new-session",
                checkpoint,
                6,
            )
        });
        let observer = store.clone();
        assert!(tokio::task::spawn_blocking(
            move || observer.wait_for_queued_mutation_for_test(Duration::from_secs(2))
        )
        .await
        .unwrap());
        assert!(!operation.is_finished());
        let available = state.svc.mux.try_write().is_some();
        if available && change_launch {
            state
                .svc
                .mux
                .write()
                .get_tab_mut(&worker)
                .unwrap()
                .agent_session_id = Some("changed-session".into());
        }
        crate::rest_liveness_tests::assert_health_list_send_respond(state.clone()).await;
        drop(held);
        let result = operation.await.unwrap();
        assert!(available, "plan writer wait must release mux");
        let current = store
            .get(&plan.id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|item| item.id == task)
            .unwrap();
        let mux = state.svc.mux.read();
        let binding = &mux.get_tab(&worker).unwrap().task_binding;
        if change_launch {
            assert!(result.unwrap_err().contains("conflict"));
            assert_eq!(current.execution_attempts, before.execution_attempts);
            assert_eq!(current.interruption, before.interruption);
            assert!(binding.is_none());
        } else {
            let (epoch, attempt_id) = result.unwrap();
            assert_eq!(current.interruption.owner_epoch, epoch);
            assert_eq!(
                current.interruption.owner_tab_id.as_deref(),
                Some(worker.as_str())
            );
            assert_eq!(current.execution_attempts.last().unwrap().id, attempt_id);
            assert_eq!(
                binding.as_ref().unwrap().attempt_id.as_deref(),
                Some(attempt_id.as_str())
            );
        }
    }
}
