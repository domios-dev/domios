use super::*;

fn prepared_store() -> (tempfile::TempDir, PlanStore, String, String, String) {
    let (dir, store, plan_id, task_id) = bound_report_store();
    store
        .update_attempt_executor_identity(
            &plan_id,
            &task_id,
            "worker",
            Some("Worker"),
            Some("session"),
            Some("nonce"),
            4,
        )
        .unwrap();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .set_attempt_plan_skill_manifest(
            &plan_id,
            &task_id,
            &attempt_id,
            vec![crate::plan::PlanSkillManifestEntry {
                slug: "dispatch-contract".into(),
                title: "Dispatch Contract".into(),
                purpose: "Worker dispatch".into(),
                content_digest: "sha256:abc".into(),
            }],
            5,
        )
        .unwrap();
    (dir, store, plan_id, task_id, attempt_id)
}

#[test]
fn plan_skill_show_records_exact_attempt_evidence_idempotently() {
    let (_dir, store, plan_id, task_id, attempt_id) = prepared_store();
    for now in [6, 7] {
        store
            .record_plan_skill_load(
                &plan_id,
                &task_id,
                "worker",
                "session",
                "nonce",
                Some(&attempt_id),
                "dispatch-contract",
                "sha256:abc",
                now,
            )
            .unwrap();
    }
    assert!(store
        .record_plan_skill_load(
            &plan_id,
            &task_id,
            "worker",
            "session",
            "nonce",
            Some("stale-attempt"),
            "dispatch-contract",
            "sha256:abc",
            8,
        )
        .is_err());
    let attempt = &store.show_task(&task_id).unwrap().task.execution_attempts[0];
    assert_eq!(attempt.loaded_plan_skills.len(), 1);
    assert_eq!(attempt.loaded_plan_skills[0].loaded_at, 6);
}

#[test]
fn competition_plan_skill_load_uses_exact_attempt_not_scalar_assignment() {
    let (_dir, store, plan_id, task_id, attempt_id) = prepared_store();
    let mut plan = store.get(&plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.assigned_tab_id = None;
    task.execution_attempts[0].competition_group_id = Some("compete-1".into());
    store.save(&mut plan, 6).unwrap();
    store
        .record_plan_skill_load(
            &plan_id,
            &task_id,
            "worker",
            "session",
            "nonce",
            Some(&attempt_id),
            "dispatch-contract",
            "sha256:abc",
            7,
        )
        .unwrap();
}

#[test]
fn report_snapshot_retains_loaded_plan_skills_after_restart() {
    let (dir, store, plan_id, task_id, attempt_id) = prepared_store();
    store
        .record_plan_skill_load(
            &plan_id,
            &task_id,
            "worker",
            "session",
            "nonce",
            Some(&attempt_id),
            "dispatch-contract",
            "sha256:abc",
            6,
        )
        .unwrap();
    store
        .record_task_attempt_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("worker"),
            "source-only handoff",
            None,
            7,
        )
        .unwrap();
    let reloaded = PlanStore::new_for_test(dir.path());
    let persisted = reloaded
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .into_iter()
        .find(|attempt| attempt.id == attempt_id)
        .unwrap();
    let report = persisted.report.as_ref().unwrap();
    assert_eq!(report.plan_skill_manifest, persisted.plan_skill_manifest);
    assert_eq!(report.loaded_plan_skills, persisted.loaded_plan_skills);
}
