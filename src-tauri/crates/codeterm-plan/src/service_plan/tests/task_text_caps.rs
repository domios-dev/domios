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
