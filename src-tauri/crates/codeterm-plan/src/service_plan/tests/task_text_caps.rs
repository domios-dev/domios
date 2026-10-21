use super::*;

#[test]
fn task_body_cap_and_control_bytes() {
    let (store, plan_id, task_id) = store_with_task(&[]);
    let oversized = "x".repeat(3 * 1024 * 1024);
    for body in [oversized, "escape\u{1b}byte".into(), "bell\u{7}byte".into()] {
        let error = store
            .add_task(
                &plan_id,
                NewTask {
                    name: "bounded input".into(),
                    kind: "implement".into(),
                    body,
                    ..Default::default()
                },
                3,
            )
            .unwrap_err();
        assert!(matches!(error, PlanError::InvalidPlanText(_)));
    }
    assert!(matches!(
        store.add_task(
            &plan_id,
            NewTask {
                name: " ".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        ),
        Err(PlanError::InvalidPlanText(_))
    ));
    assert_eq!(store.get(&plan_id).unwrap().tasks.len(), 1);

    let section_id = store
        .add_section(
            &plan_id,
            "bounded section",
            "body",
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();
    assert!(matches!(
        store.update_section(
            &plan_id,
            &section_id,
            None,
            Some("escape\u{1b}body"),
            &[],
            None,
            None,
            5,
        ),
        Err(PlanError::InvalidPlanText(_))
    ));
    assert!(matches!(
        store.add_milestone(
            &plan_id,
            &task_id,
            "status_update",
            "bell\u{7}text",
            AgentRef::default(),
            false,
            6
        ),
        Err(PlanError::InvalidPlanText(_))
    ));
    assert_eq!(store.get(&plan_id).unwrap().sections[0].body, "body");
}

#[test]
fn stored_legacy_control_bytes_do_not_block_later_writes() {
    let (store, plan_id, task_id) = store_with_task(&["status_update"]);
    store.plans.read().get(&plan_id).unwrap().lock().tasks[0].test_plan =
        Some("\u{8}ash scripts/check".into());

    store
        .update_task(
            &plan_id,
            &task_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("legacy-group")),
            &[],
            None,
            None,
            3,
        )
        .unwrap();
    store
        .add_milestone(
            &plan_id,
            &task_id,
            "status_update",
            "plain text",
            AgentRef::default(),
            false,
            4,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks[0].group.as_deref(), Some("legacy-group"));
    assert_eq!(
        plan.tasks[0].test_plan.as_deref(),
        Some("\u{8}ash scripts/check")
    );
    assert!(matches!(
        store.add_milestone(
            &plan_id,
            &task_id,
            "status_update",
            "bell\u{7}text",
            AgentRef::default(),
            false,
            5
        ),
        Err(PlanError::InvalidPlanText(_))
    ));
}

#[test]
fn a_long_lived_plan_with_many_stored_fields_still_accepts_writes() {
    let (store, plan_id, task_id) = store_with_task(&["status_update"]);
    {
        let plans = store.plans.read();
        let mut plan = plans.get(&plan_id).unwrap().lock();
        let template = plan.tasks[0].clone();
        for index in 0..80 {
            let mut task = template.clone();
            task.id = format!("task_bulk{index}");
            task.tags = (0..1000).map(|tag| format!("t{tag}")).collect();
            plan.tasks.push(task);
        }
    }

    store
        .add_milestone(
            &plan_id,
            &task_id,
            "status_update",
            "plain text",
            AgentRef::default(),
            false,
            3,
        )
        .unwrap();
    assert_eq!(store.get(&plan_id).unwrap().tasks.len(), 81);
}
