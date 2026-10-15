use super::*;

#[test]
fn split_creates_children_with_parent_and_inherited_routing() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Split parent".into(),
                kind: "implement".into(),
                model: Some("codex:gpt-6-sol".into()),
                model_rationale: Some("precise edits".into()),
                personality: Some("Nova".into()),
                role_guidance: Some("Own the Rust layer.".into()),
                executor_role: Some("rust-backend-dev".into()),
                assigned_tab_id: Some("tab-parent-worker".into()),
                lane: TaskLaneMetadata {
                    tab_id: Some("tab-parent-worker".into()),
                    ..Default::default()
                },
                body: "Shared context".into(),
                acceptance_criteria: vec!["works".into()],
                test_plan: Some("focused test".into()),
                correctness: Some("preserve isolation".into()),
                tags: vec!["area:plan".into(), "topic".into()],
                ..Default::default()
            },
            5,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .with_plan(&plan_id, 6, |plan| {
            let parent = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == parent_id)
                .unwrap();
            parent.status = TaskStatus::InProgress;
            parent.status_changed_at = 6;
            Ok(())
        })
        .unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id.clone());
    params["children"] = json!([
        {"name":"Part A"},
        {"name":"Part B", "body":"Part-specific context", "criteria":["part works"], "test_plan":"verify this part", "tags":["area:ui"]}
    ]);

    let response = dispatch("plan.task.split", &Some(params), &service, 10).unwrap();
    assert_eq!(response["parent_task_id"], parent_id);
    let children = response["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    let parent = plan.tasks.iter().find(|task| task.id == parent_id).unwrap();
    assert_eq!(parent.status, TaskStatus::InProgress);
    assert_eq!(parent.assigned_tab_id.as_deref(), Some("tab-parent-worker"));
    assert_eq!(parent.lane.tab_id.as_deref(), Some("tab-parent-worker"));
    for (index, created) in children.iter().enumerate() {
        let id = created["task_id"].as_str().unwrap();
        let child = plan.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(child.parent_task_id.as_deref(), Some(parent_id.as_str()));
        assert_eq!(child.executor_role.as_deref(), Some("rust-backend-dev"));
        assert_eq!(child.personality.as_deref(), Some("Nova"));
        assert_eq!(child.model.as_deref(), Some("codex:gpt-6-sol"));
        assert!(child.assigned_tab_id.is_none());
        assert!(child.lane.tab_id.is_none());
        if index == 0 {
            assert_eq!(child.body, "Shared context");
            assert_eq!(child.acceptance_criteria, vec!["works".to_string()]);
            assert_eq!(child.test_plan.as_deref(), Some("focused test"));
            assert_eq!(child.tags, ["area:plan", "topic"]);
        } else {
            assert_eq!(child.body, "Part-specific context");
            assert_eq!(child.acceptance_criteria, vec!["part works".to_string()]);
            assert_eq!(child.test_plan.as_deref(), Some("verify this part"));
            assert_eq!(child.tags, ["topic", "area:ui"]);
        }
    }
}

#[test]
fn split_invalid_second_child_is_atomic() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Parent".into(),
                executor_role: Some("rust-backend-dev".into()),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    let before = fixture
        .service
        .plan_store
        .get(&plan_id)
        .unwrap()
        .tasks
        .len();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id);
    params["children"] =
        json!([{"name":"Valid"}, {"name":"Invalid", "sections":"missing-section"}]);
    assert!(dispatch("plan.task.split", &Some(params), &service, 10).is_err());
    assert_eq!(
        fixture
            .service
            .plan_store
            .get(&plan_id)
            .unwrap()
            .tasks
            .len(),
        before
    );
}

#[test]
fn split_rejects_terminal_and_child_parents() {
    for status in [Some(TaskStatus::Done), Some(TaskStatus::Cancelled), None] {
        let expected_reason = if status.is_some() {
            "terminal_parent"
        } else {
            "child_parent"
        };
        let (fixture, plan_id, _, _, _, owner) = update_fixture();
        let parent_id = fixture
            .service
            .plan_store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Parent".into(),
                    executor_role: Some("rust-backend-dev".into()),
                    ..Default::default()
                },
                5,
            )
            .unwrap();
        let target_id = if let Some(status) = status {
            fixture
                .service
                .plan_store
                .with_plan(&plan_id, 6, |plan| {
                    plan.tasks
                        .iter_mut()
                        .find(|task| task.id == parent_id)
                        .unwrap()
                        .status = status;
                    Ok(())
                })
                .unwrap();
            parent_id
        } else {
            fixture
                .service
                .plan_store
                .add_task(
                    &plan_id,
                    NewTask {
                        name: "Child".into(),
                        parent_task_id: Some(parent_id),
                        ..Default::default()
                    },
                    6,
                )
                .unwrap()
        };
        let service = Arc::new(fixture.service.clone());
        let mut params = serde_json::Value::Object(owner_params(&owner));
        params["plan_id"] = json!(plan_id);
        params["task_id"] = json!(target_id);
        params["children"] = json!([{"name":"Nested"}]);
        let error = dispatch("plan.task.split", &Some(params), &service, 10).unwrap_err();
        assert_eq!(error.code, -32602);
        assert_eq!(error.data.as_ref().unwrap()["type"], "invalid_split_parent");
        assert_eq!(error.data.as_ref().unwrap()["reason"], expected_reason);
    }
}

#[test]
fn task_update_cancel_parent_with_open_child_returns_open_subtasks() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Parent".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Child".into(),
                parent_task_id: Some(parent_id.clone()),
                ..Default::default()
            },
            6,
        )
        .unwrap();
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id.clone());
    params["status"] = json!("cancelled");
    let service = Arc::new(fixture.service.clone());
    let error = dispatch("plan.task.update", &Some(params), &service, 10).unwrap_err();
    assert_eq!(error.data.as_ref().unwrap()["type"], "open_subtasks");
    assert_eq!(
        fixture
            .service
            .plan_store
            .show_task(&parent_id)
            .unwrap()
            .task
            .status,
        TaskStatus::Pending
    );
}

#[test]
fn draft_split_children_remain_locked_until_plan_approval() {
    let (fixture, plan_id, _, _, _, owner) = update_fixture();
    let parent_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Parent".into(),
                executor_role: Some("rust-backend-dev".into()),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    let service = Arc::new(fixture.service.clone());
    let mut params = serde_json::Value::Object(owner_params(&owner));
    params["plan_id"] = json!(plan_id.clone());
    params["task_id"] = json!(parent_id);
    params["children"] = json!([{"name":"Child"}]);
    let response = dispatch("plan.task.split", &Some(params), &service, 10).unwrap();
    let child_id = response["children"][0]["task_id"].as_str().unwrap();
    let caller = codeterm_service_core::service::AuthenticatedPlanCaller {
        tab_id: Some(owner),
        session_id: Some("owner-session".into()),
        binding_nonce: Some("owner-nonce".into()),
    };
    assert!(fixture
        .service
        .authorize_execution_action(&plan_id, &caller)
        .is_err());
    fixture
        .service
        .plan_store
        .set_planning_state(
            &plan_id,
            codeterm_service_core::plan::PlanningState::Approved,
            codeterm_service_core::plan::PlanningAuthor::User,
            Some("approve split children"),
            12,
        )
        .unwrap();
    assert!(fixture
        .service
        .authorize_execution_action(&plan_id, &caller)
        .is_ok());
    assert_eq!(
        fixture
            .service
            .plan_store
            .show_task(child_id)
            .unwrap()
            .task
            .status,
        TaskStatus::Pending
    );
}
