use super::*;

fn team(id: &str) -> Team {
    Team {
        locked: false,
        id: id.into(),
        name: Some("Board".into()),
        lifecycle: codeterm_types::team::TeamLifecycle::Active,
        created_at: 1,
        workspace_id: "workspace".into(),
    }
}

fn plan(
    id: &str,
    team_id: &str,
    status: PlanStatus,
    updated_at: u64,
    lifecycle_events: Vec<PlanLifecycleEvent>,
    stored_kind: Option<&str>,
) -> Plan {
    let mut plan = Plan::new(
        format!("Plan {id}"),
        "/repo".into(),
        codeterm_types::plan::OrchestratorRef::from_tab("author-tab"),
    );
    plan.id = id.into();
    plan.team_id = Some(team_id.into());
    plan.status = status;
    plan.created_at = updated_at.saturating_sub(10);
    plan.updated_at = updated_at;
    plan.lifecycle_events = lifecycle_events;
    if let Some(kind) = stored_kind {
        plan.strides.push(codeterm_types::plan::Milestone {
            id: format!("milestone-{id}"),
            from: codeterm_types::plan::OrchestratorRef::from_tab("worker-tab"),
            kind: kind.into(),
            text: format!("stored {id}"),
            ts: updated_at,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
    }
    plan
}

fn lifecycle(kind: &str, at: u64, id: &str) -> PlanLifecycleEvent {
    PlanLifecycleEvent {
        id: id.into(),
        kind: kind.into(),
        at,
        author: codeterm_types::plan::OrchestratorRef::from_tab("author-tab"),
    }
}

#[test]
fn team_board_state_lifecycle_events_are_recorded() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Team board".into(),
        root.path().display().to_string(),
        codeterm_types::plan::OrchestratorRef::from_tab("owner-tab"),
    );
    plan.team_id = Some("team-a".into());
    store.save(&mut plan, 10).unwrap();
    store.archive(&plan.id, 20).unwrap();
    store.unarchive(&plan.id, 30).unwrap();

    let events = store.get(&plan.id).unwrap().lifecycle_events;
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["opened", "activated", "archived", "activated"]
    );
}

#[test]
fn team_board_state_active_and_lifecycle_projection() {
    let active = plan(
        "active",
        "team-a",
        PlanStatus::Active,
        30,
        vec![
            lifecycle("opened", 10, "open"),
            lifecycle("activated", 20, "activate"),
        ],
        Some("stored"),
    );
    let archived = plan(
        "archived",
        "team-a",
        PlanStatus::Archived,
        40,
        vec![lifecycle("archived", 40, "archive")],
        None,
    );
    let state = project_team_state(team("team-a"), vec![active, archived], 20, None).unwrap();
    assert_eq!(state.team_id, "team-a");
    assert_eq!(
        state.active_plan.as_ref().map(|plan| plan.id.as_str()),
        Some("active")
    );
    assert_eq!(state.archived_plans.len(), 1);
    assert!(state.events.iter().any(|event| event.kind == "stored"));
    assert!(state.events.iter().any(|event| event.kind == "opened"));
    assert!(state.events.iter().any(|event| event.kind == "activated"));
    assert!(state.events.iter().any(|event| event.kind == "archived"));
}

#[test]
fn team_board_state_no_active_plan_keeps_archived_plans() {
    let archived = plan(
        "archived",
        "team-a",
        PlanStatus::Archived,
        10,
        vec![lifecycle("archived", 10, "archive")],
        None,
    );
    let state = project_team_state(team("team-a"), vec![archived], 20, None).unwrap();
    assert!(state.active_plan.is_none());
    assert_eq!(state.archived_plans[0].id, "archived");
}

#[test]
fn team_board_state_excludes_foreign_plans() {
    let local = plan("local", "team-a", PlanStatus::Active, 20, vec![], None);
    let foreign = plan(
        "foreign",
        "team-b",
        PlanStatus::Active,
        30,
        vec![],
        Some("foreign"),
    );
    let foreign_id = foreign.id.clone();
    let state = project_team_state(team("team-a"), vec![local, foreign], 20, None).unwrap();
    assert_eq!(
        state.active_plan.as_ref().map(|plan| plan.id.as_str()),
        Some("local")
    );
    assert!(state.events.iter().all(|event| event.plan_id != foreign_id));
}

#[test]
fn team_board_state_pages_newest_first_and_bounded() {
    let plans = vec![
        plan(
            "a",
            "team-a",
            PlanStatus::Active,
            10,
            vec![],
            Some("stored-a"),
        ),
        plan(
            "b",
            "team-a",
            PlanStatus::Archived,
            20,
            vec![lifecycle("archived", 19, "archive")],
            Some("stored-b"),
        ),
    ];
    let state = project_team_state(team("team-a"), plans, 2, None).unwrap();
    assert_eq!(state.events.len(), 2);
    assert_eq!(state.events[0].ts, 20);
    assert_eq!(state.events[1].ts, 19);
    assert!(state
        .events
        .windows(2)
        .all(|window| compare_events(&window[0], &window[1]) != Ordering::Greater));
    assert!(state.next_cursor.is_some());
}

#[test]
fn team_board_state_cursor_continues_without_duplicates() {
    let plans = vec![
        plan(
            "a",
            "team-a",
            PlanStatus::Active,
            10,
            vec![],
            Some("stored-a"),
        ),
        plan(
            "b",
            "team-a",
            PlanStatus::Archived,
            20,
            vec![lifecycle("archived", 19, "archive")],
            Some("stored-b"),
        ),
    ];
    let first = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
    let second = project_team_state(
        team("team-a"),
        plans.clone(),
        1,
        first.next_cursor.as_deref(),
    )
    .unwrap();
    assert_eq!(second.events.len(), 1);
    let first_ids: std::collections::BTreeSet<_> =
        first.events.iter().map(|event| event.id.as_str()).collect();
    assert!(second
        .events
        .iter()
        .all(|event| !first_ids.contains(event.id.as_str())));
}

#[test]
fn team_board_state_cursor_is_deterministic() {
    let plans = vec![plan(
        "a",
        "team-a",
        PlanStatus::Active,
        10,
        vec![lifecycle("opened", 2, "open")],
        Some("stored"),
    )];
    let first = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
    let second = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
    assert_eq!(first, second);
    let cursor = first.next_cursor.as_deref().unwrap();
    let page_a = project_team_state(team("team-a"), plans.clone(), 1, Some(cursor));
    let page_b = project_team_state(team("team-a"), plans, 1, Some(cursor));
    assert_eq!(page_a, page_b);
}

#[test]
fn team_board_summary_size_does_not_grow_with_task_records() {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut record = plan("p", "team-a", PlanStatus::Active, 10, vec![], None);
    store.save(&mut record, 10).unwrap();
    let task_id = store
        .add_task(
            &record.id,
            codeterm_plan::service_plan::NewTask {
                name: "Task".into(),
                ..Default::default()
            },
            11,
        )
        .unwrap();
    let mut record = store.get(&record.id).unwrap();
    let task = record
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap()
        .clone();
    let small = serde_json::to_vec(&TeamPlanSummary::from(&record)).unwrap();
    record.tasks = (0..596)
        .map(|index| {
            let mut task = task.clone();
            task.id = format!("task-{index}");
            task.body = "payload".repeat(5_000);
            task
        })
        .collect();
    let summary = TeamPlanSummary::from(&record);
    assert_eq!(summary.task_counts.total, 596);
    assert_eq!(summary.task_counts.pending, 596);
    let large = serde_json::to_vec(&summary).unwrap();
    assert!(large.len() <= small.len() + 32);
    let json = serde_json::to_value(summary).unwrap();
    assert!(json.get("tasks").is_none());
    assert!(json.get("execution_attempts").is_none());
    assert!(json.get("milestones").is_none());
    assert_eq!(json["id"], "p");
    assert_eq!(json["title"], "Plan p");
}

#[test]
fn team_board_summary_carries_only_the_displayed_goal() {
    let mut record = plan("p", "team-a", PlanStatus::Active, 10, vec![], None);
    record.goal = Some(codeterm_types::plan::Goal {
        vector: "Visible goal".into(),
        vision: "private planning prose".repeat(1000),
        constraints: vec!["constraint".repeat(1000)],
        postulates: vec!["postulate".repeat(1000)],
    });
    let json = serde_json::to_value(TeamPlanSummary::from(&record)).unwrap();
    assert_eq!(
        json["goal"],
        serde_json::json!({ "vector": "Visible goal" })
    );
}

#[test]
fn team_board_preserves_folded_checklist_tasks_in_paged_projection() {
    let mut plan = plan("fixture", "team-a", PlanStatus::Active, 100, vec![], None);
    for index in 0..60 {
        let mut task: codeterm_types::plan::Task = serde_json::from_value(serde_json::json!({
            "id": format!("task_{index}"), "name": format!("opaque {index}"), "type": "code", "status": "in_progress"
        })).unwrap();
        task.created_at = 1;
        task.milestones.push(codeterm_types::plan::Milestone {
            id: format!("link_{index}"),
            kind: "tab_todo_linked".into(),
            text: "opaque checklist".into(),
            ts: 200 + index,
            from: Default::default(),
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
        plan.tasks.push(task);
    }
    let page = project_team_state(team("team-a"), vec![plan], 50, None).unwrap();
    let linked: Vec<_> = page
        .events
        .iter()
        .filter(|event| event.kind == "tab_todo_linked")
        .collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].count, 60);
    assert_eq!(linked[0].linked_tasks.len(), 60);
    let wire = serde_json::to_value(linked[0]).unwrap();
    assert_eq!(wire["linked_tasks"].as_array().unwrap().len(), 60);
}
