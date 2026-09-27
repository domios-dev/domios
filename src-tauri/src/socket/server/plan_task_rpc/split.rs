use super::*;

pub(super) fn dispatch_split(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let (plan_id, parent_id) = plan_refs::resolve_task(store, params)?;
    let parent = store
        .show_task(&parent_id)
        .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;
    if parent.task.parent_task_id.is_some() {
        return Err(plan_rpc_err(PlanError::InvalidSplitParent {
            reason: crate::service::plan::SplitParentReason::ChildParent,
        }));
    }
    if matches!(
        &parent.task.status,
        codeterm_types::plan::TaskStatus::Done | codeterm_types::plan::TaskStatus::Cancelled
    ) {
        return Err(plan_rpc_err(PlanError::InvalidSplitParent {
            reason: crate::service::plan::SplitParentReason::TerminalParent,
        }));
    }
    let children = p
        .and_then(|value| value.get("children"))
        .and_then(Value::as_array)
        .ok_or(JsonRpcError {
            code: -32602,
            message: "children must be a non-empty array".into(),
            data: Some(json!({"type":"invalid_task_split"})),
        })?;
    if children.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "split requires at least one child".into(),
            data: Some(json!({"type":"invalid_task_split"})),
        });
    }
    let author = svc
        .authorize_draft_structure_mutation(&plan_id, &authenticated_caller(p))
        .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({"type":"plan_structure_authorization_required"})),
        })?;
    if matches!(&author, crate::plan::PlanningAuthor::Planner { .. }) {
        return Err(JsonRpcError {
            code: -32003,
            message: "only the Plan Orchestrator may split a task".into(),
            data: Some(json!({"type":"plan_role_authorization_required"})),
        });
    }
    let mut additions = Vec::with_capacity(children.len());
    for child in children {
        let name = child
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or(JsonRpcError {
                code: -32602,
                message: "each child requires a non-empty name".into(),
                data: Some(json!({"type":"invalid_task_split"})),
            })?;
        crate::service::ensure_planner_task_fields_are_structural(&author, child).map_err(
            |message| JsonRpcError {
                code: -32003,
                message,
                data: Some(json!({"type":"plan_role_authorization_required"})),
            },
        )?;
        let string = |key: &str, fallback: Option<&str>| {
            child
                .get(key)
                .and_then(Value::as_str)
                .or(fallback)
                .map(str::to_string)
        };
        let criteria = child
            .get("criteria")
            .or_else(|| child.get("acceptance_criteria"))
            .cloned()
            .unwrap_or_else(|| json!(parent.task.acceptance_criteria));
        let sections = child
            .get("sections")
            .cloned()
            .unwrap_or_else(|| json!(parent.task.section_ids));
        let sections = match sections {
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","),
            Value::String(value) => value,
            _ => String::new(),
        };
        let child_params = json!({
            "plan_id": plan_id,
            "name": name,
            "parent": parent_id,
            "type": parent.task.kind,
            "body": string("body", Some(&parent.task.body)).unwrap_or_default(),
            "acceptance_criteria": criteria,
            "test_plan": string("test_plan", parent.task.test_plan.as_deref()),
            "qa_plan": string("qa_plan", parent.task.qa_plan.as_deref()),
            "correctness": string("correctness", parent.task.correctness.as_deref()),
            "sections": sections,
            "executor_role": string("executor_role", parent.task.executor_role.as_deref()),
            "personality": string("personality", parent.task.personality.as_deref()).unwrap_or_else(|| "Worker".into()),
            "role_guidance": string("role_guidance", parent.task.role_guidance.as_deref()),
            "model": string("model", parent.task.model.as_deref()),
            "why": string("model_rationale", parent.task.model_rationale.as_deref()),
            "intent": string("intent", parent.task.intent.as_deref()),
            "caller_tab_id": p.and_then(|v| v.get("caller_tab_id")),
            "session_id": p.and_then(|v| v.get("session_id")),
            "session_binding_nonce": p.and_then(|v| v.get("session_binding_nonce")),
        });
        if child_params["executor_role"].is_null() {
            return Err(JsonRpcError {
                code: -32602,
                message: "parent or child must provide executor_role".into(),
                data: Some(json!({"type":"invalid_task_split"})),
            });
        }
        validate_model_field(Some(&child_params), &svc.agent_registry.provider_ids())?;
        additions.push(child_params);
    }
    let mut new_children = Vec::with_capacity(additions.len());
    for child in &additions {
        let sections = csv_string_field(Some(child), "sections");
        let section_ids = store
            .resolve_task_section_ids(&plan_id, &sections)
            .map_err(plan_rpc_err)?;
        for section in &sections {
            if store
                .resolve_task_section_ids(&plan_id, std::slice::from_ref(section))
                .map_err(plan_rpc_err)?
                .is_empty()
            {
                return Err(JsonRpcError {
                    code: -32602,
                    message: "child references an unknown section".into(),
                    data: Some(json!({"type":"invalid_task_split"})),
                });
            }
        }
        new_children.push(NewTask {
            name: get_string_param(&Some(child.clone()), "name")?,
            kind: child
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("implement")
                .to_string(),
            section_ids,
            model: child
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            model_rationale: child.get("why").and_then(Value::as_str).map(str::to_string),
            personality: child
                .get("personality")
                .and_then(Value::as_str)
                .map(str::to_string),
            acceptance_criteria: string_array_field(Some(child), "acceptance_criteria"),
            test_plan: child
                .get("test_plan")
                .and_then(Value::as_str)
                .map(str::to_string),
            qa_plan: child
                .get("qa_plan")
                .and_then(Value::as_str)
                .map(str::to_string),
            correctness: child
                .get("correctness")
                .and_then(Value::as_str)
                .map(str::to_string),
            body: child
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            role_guidance: child
                .get("role_guidance")
                .and_then(Value::as_str)
                .map(str::to_string),
            executor_role: child
                .get("executor_role")
                .and_then(Value::as_str)
                .map(str::to_string),
            intent: child
                .get("intent")
                .and_then(Value::as_str)
                .map(str::to_string),
            ..Default::default()
        });
    }
    let ids = store
        .add_child_tasks_atomic(&plan_id, &parent_id, new_children, now)
        .map_err(plan_rpc_err)?;
    let created = ids.into_iter().map(|id| json!({
        "task_id": id,
        "key": store.task_display_key(&plan_id, &id),
        "section_ids": store.get(&plan_id).and_then(|plan| plan.tasks.iter().find(|task| task.id == id).map(|task| task.section_ids.clone())).unwrap_or_default(),
    })).collect::<Vec<_>>();
    emit_plan_changed(app, &plan_id);
    Ok(json!({"plan_id": plan_id, "parent_task_id": parent_id, "children": created}))
}
