use super::*;

#[test]
fn bounded_detail_matches_the_full_fixture_reference_for_graph_and_presentation() {
    let directory = tempfile::tempdir().unwrap();
    let store = crate::service_plan::PlanStore::new_for_test(directory.path());
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].section_ids = vec![];
    let mut dependency = plan.tasks[0].clone();
    dependency.id = "dependency".into();
    dependency.key = None;
    dependency.handle = None;
    dependency.status = TaskStatus::Pending;
    let mut child = dependency.clone();
    child.id = "child".into();
    child.parent_task_id = Some("task_exact".into());
    plan.tasks.push(dependency);
    plan.tasks.push(child);
    plan.task_groups=serde_json::from_value(serde_json::json!([
        {"key":"earlier","label":"Earlier"},{"key":"later","label":"Later"},{"key":"later","label":"Repeated"}
    ])).unwrap();
    for status in [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::InReview,
        TaskStatus::Blocked,
        TaskStatus::Paused,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ] {
        for scenario in 0..5 {
            plan.tasks[0].status = status.clone();
            plan.tasks[0].depends_on = if scenario == 0 {
                vec!["dependency".into(), "missing".into(), "dependency".into()]
            } else {
                vec![]
            };
            plan.tasks[0].interruption.handoff_required = scenario >= 2;
            plan.tasks[0].group = (scenario >= 3).then(|| "later".into());
            plan.tasks[1].group = Some("earlier".into());
            plan.tasks[1].status = if scenario == 4 {
                TaskStatus::Done
            } else {
                TaskStatus::Pending
            };
            plan.tasks[2].status = if scenario == 1 {
                TaskStatus::Pending
            } else {
                TaskStatus::Done
            };
            store.save(&mut plan, 1 + scenario).unwrap();
            let expected = store.inspect_task_for_test("task_exact").unwrap();
            for reference in ["task_exact", "NAMED", "cod-1"] {
                let actual = store.show_task_window(reference).unwrap();
                assert_eq!(
                    serde_json::to_vec(&actual).unwrap(),
                    serde_json::to_vec(&expected).unwrap(),
                    "status={status:?} scenario={scenario}"
                );
            }
        }
    }
    assert!(matches!(
        store.show_task_window("missing"),
        Err(PlanError::TaskNotFound)
    ));
}
