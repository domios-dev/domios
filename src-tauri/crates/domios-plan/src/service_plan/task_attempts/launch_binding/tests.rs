use super::*;
use crate::service_plan::plan_skills::{create_plan_skill, plan_skill_manifest, NewPlanSkill};

fn prepared() -> (
    tempfile::TempDir,
    PlanStore,
    CompleteAcknowledgedDispatchBinding,
) {
    let (dir, store, plan_id, task_id) =
        crate::service_plan::task_attempt_tests::bound_report_store();
    crate::service_plan::paths::mark_test_project_root(dir.path());
    store
        .with_plan(&plan_id, 4, |plan| {
            plan.team_id = Some("launch-team".into());
            Ok(())
        })
        .unwrap();
    let attempt_id = store
        .begin_task_dispatch_with_policy(
            &plan_id,
            &task_id,
            "worker",
            "Worker",
            crate::plan::TaskLaneMetadata::default(),
            crate::plan::ExecutionIntent::Unspecified,
            None,
            Some("provider"),
            5,
        )
        .unwrap();
    create_plan_skill(
        &store,
        &plan_id,
        NewPlanSkill {
            slug: "launch-contract".into(),
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let manifest = plan_skill_manifest(&store, &plan_id).unwrap();
    store
        .set_attempt_plan_skill_manifest(&plan_id, &task_id, &attempt_id, manifest, 6)
        .unwrap();
    let ack = store
        .acknowledge_task_attempt(&plan_id, &task_id, "worker", &attempt_id, 7)
        .unwrap();
    let request = CompleteAcknowledgedDispatchBinding {
        plan_id,
        task_id,
        attempt_id,
        tab_id: "worker".into(),
        team_id: "launch-team".into(),
        creator_tab_id: "orch".into(),
        provider: "provider".into(),
        session_id: "launch-session".into(),
        nonce: "launch-nonce".into(),
        owner_epoch: ack.attempt.interruption.owner_epoch,
    };
    (dir, store, request)
}

#[test]
fn acknowledged_launch_binding_changes_only_missing_identity_and_replay_is_no_write() {
    let (_dir, store, request) = prepared();
    let mut expected = store.show_task(&request.task_id).unwrap().task;
    let index = expected.execution_attempts.len() - 1;
    expected.execution_attempts[index].session_id = Some(request.session_id.clone());
    expected.execution_attempts[index].session_binding_nonce = Some(request.nonce.clone());
    expected.interruption.owner_session_id = Some(request.session_id.clone());
    let completed = store
        .complete_acknowledged_dispatch_binding(request.clone(), 8)
        .unwrap();
    assert_eq!(completed, expected.execution_attempts[index]);
    assert_eq!(store.show_task(&request.task_id).unwrap().task, expected);
    let persisted = store.get(&request.plan_id).unwrap();
    assert_eq!(
        store
            .complete_acknowledged_dispatch_binding(request.clone(), 9)
            .unwrap(),
        completed
    );
    assert_eq!(store.get(&request.plan_id).unwrap(), persisted);
}

#[test]
fn acknowledged_launch_binding_refuses_partial_foreign_owner_and_cross_task_ambiguity() {
    for variant in ["partial", "foreign", "owner", "other_task"] {
        let (_dir, store, request) = prepared();
        if variant == "other_task" {
            let other = store
                .add_task(
                    &request.plan_id,
                    crate::service_plan::NewTask {
                        name: "foreign assignment".into(),
                        kind: "task".into(),
                        ..Default::default()
                    },
                    8,
                )
                .unwrap();
            store
                .assign_task(&request.plan_id, &other, "worker", None, 9)
                .unwrap();
        } else {
            store
                .with_plan(&request.plan_id, 8, |plan| {
                    let task = plan
                        .tasks
                        .iter_mut()
                        .find(|task| task.id == request.task_id)
                        .unwrap();
                    let attempt = task.execution_attempts.last_mut().unwrap();
                    match variant {
                        "partial" => attempt.session_id = Some(request.session_id.clone()),
                        "foreign" => {
                            attempt.session_id = Some("foreign".into());
                            attempt.session_binding_nonce = Some("foreign".into());
                        }
                        "owner" => task.interruption.owner_session_id = Some("foreign".into()),
                        _ => unreachable!(),
                    }
                    Ok(())
                })
                .unwrap();
        }
        let before = store.get(&request.plan_id).unwrap();
        assert!(
            store
                .complete_acknowledged_dispatch_binding(request.clone(), 10)
                .is_err(),
            "{variant}"
        );
        assert_eq!(store.get(&request.plan_id).unwrap(), before, "{variant}");
    }
}
