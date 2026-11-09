use super::*;
use crate::plan::AttemptBindingRestorationReceipt;
use crate::service_plan::plan_skills::{create_plan_skill, plan_skill_manifest, NewPlanSkill};
use crate::service_plan::task_attempts::{
    CompleteAcknowledgedDispatchBinding, RestoreCurrentAttemptBinding,
};
use sha2::Digest;

fn digest(value: &str) -> String {
    format!("sha256:{:x}", sha2::Sha256::digest(value.as_bytes()))
}

#[test]
fn launch_skill_and_restore_bindings_are_bounded_and_rollback_owned_fields() {
    let mut baseline = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixture(root.path(), strides, attempts);
        crate::service_plan::paths::mark_test_project_root(root.path());
        plan.team_id = Some("team".into());
        let task = &mut plan.tasks[0];
        task.status = TaskStatus::InProgress;
        task.assigned_tab_id = Some("worker".into());
        task.lane.tab_id = Some("worker".into());
        task.interruption.owner_epoch = 1;
        task.interruption.owner_tab_id = Some("worker".into());
        let attempt = &mut task.execution_attempts[0];
        attempt.session_id = None;
        attempt.session_binding_nonce = None;
        attempt.provider = Some("provider".into());
        attempt.agent_type = Some("provider".into());
        attempt.role = Some("worker".into());
        attempt.acknowledged_at = Some(2);
        attempt.interruption.owner_epoch = 1;
        drop(database(root.path(), &plan));
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        create_plan_skill(
            &store,
            &plan.id,
            NewPlanSkill {
                slug: "binding".into(),
                title: None,
                area: None,
                check: None,
                rationale: None,
            },
        )
        .unwrap();
        let manifest = plan_skill_manifest(&store, &plan.id).unwrap();
        assert!(!manifest.is_empty());
        store
            .set_attempt_plan_skill_manifest(&plan.id, "task_0000", "current", manifest.clone(), 20)
            .unwrap();
        let request = CompleteAcknowledgedDispatchBinding {
            plan_id: plan.id.clone(),
            task_id: "task_0000".into(),
            attempt_id: "current".into(),
            tab_id: "worker".into(),
            team_id: "team".into(),
            creator_tab_id: "owner".into(),
            provider: "provider".into(),
            session_id: "session".into(),
            nonce: "nonce".into(),
            owner_epoch: 1,
        };
        load_audit::start(&store);
        let mut work = Vec::new();
        let undo = store
            .complete_acknowledged_dispatch_binding_with_undo(request, 30)
            .unwrap();
        work.push(take_work());
        store
            .record_plan_skill_load(
                &plan.id,
                "task_0000",
                "worker",
                "session",
                "nonce",
                Some("current"),
                &manifest[0].slug,
                &manifest[0].content_digest,
                31,
            )
            .unwrap();
        work.push(take_work());
        assert_eq!(
            store
                .announce_current_plan_skill_manifest(
                    &plan.id,
                    "task_0000",
                    "current",
                    "worker",
                    "session",
                    "nonce",
                    32
                )
                .unwrap(),
            manifest
        );
        assert_eq!(&take_work()[..5], &[0; 5]);
        let restore = RestoreCurrentAttemptBinding {
            plan_id: plan.id.clone(),
            task_id: "task_0000".into(),
            attempt_id: "current".into(),
            executor_tab_id: "worker".into(),
            session_id: "session".into(),
            provider: "provider".into(),
            team_id: "team".into(),
            owner_epoch: 1,
            expected_old_nonce: "nonce".into(),
            current_nonce: "new-nonce".into(),
            receipt: AttemptBindingRestorationReceipt {
                operation_id: "restore".into(),
                restored_at: 33,
                attempt_id: "current".into(),
                executor_tab_id: "worker".into(),
                session_id: "session".into(),
                provider: "provider".into(),
                owner_epoch: 1,
                old_nonce_digest: digest("nonce"),
                current_nonce_digest: digest("new-nonce"),
                lineage_proof_kind: "prior_session_binding_nonce_suffix".into(),
                lineage_proof_digest: digest("lineage"),
            },
        };
        store
            .restore_current_attempt_binding(restore.clone(), 33)
            .unwrap();
        work.push(take_work());
        assert!(store
            .record_plan_skill_load(
                &plan.id,
                "task_0000",
                "worker",
                "session",
                "nonce",
                Some("current"),
                &manifest[0].slug,
                &manifest[0].content_digest,
                34
            )
            .is_err());
        store.undo_restored_attempt_binding(&restore, 35).unwrap();
        work.push(take_work());
        store.undo_completed_dispatch_binding(undo, 36).unwrap();
        work.push(take_work());
        assert!(store.plan_loads_for_test().is_empty());
        for receipt in &work {
            assert!(receipt[0] <= 1 && receipt[1] == 1 && receipt[2] == 1);
            assert_eq!(&receipt[3..5], &[0, 0]);
            assert!(receipt[5] < 64 * 1024 && receipt[6] < 64 * 1024);
        }
        println!("fixture={SEED} strides={strides} unrelated_attempts={attempts} binding_receipts={work:?}");
        if let Some(baseline) = &baseline {
            assert_eq!(baseline, &work);
        }
        baseline = Some(work);
        let persisted = store.database.load_plan(&plan.id).unwrap().unwrap();
        let attempt = &persisted.tasks[0].execution_attempts[0];
        assert!(attempt.session_id.is_none() && attempt.session_binding_nonce.is_none());
        assert_eq!(attempt.loaded_plan_skills.len(), 1);
        assert!(attempt.binding_restorations.is_empty());
    }
}
