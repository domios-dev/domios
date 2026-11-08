use super::*;

pub(super) fn dispatch_split(
    params: &Option<Value>,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let p = params.as_ref();
    let store = &svc.plan_store;
    let (plan_id, parent_id) = plan_refs::resolve_task(store, params)?;
    let include_body = p
        .and_then(|p| p.get("children"))
        .and_then(Value::as_array)
        .is_some_and(|children| {
            children
                .iter()
                .any(|child| child.get("body").and_then(Value::as_str).is_none())
        });
    let parent = store
        .task_split_defaults_projection(&parent_id, include_body)
        .map_err(plan_rpc_err)?;
    if parent.parent_task_id.is_some() {
        return Err(plan_rpc_err(PlanError::InvalidSplitParent {
            reason: codeterm_service_core::service::plan::SplitParentReason::ChildParent,
        }));
    }
    if matches!(
        &parent.status,
        codeterm_types::plan::TaskStatus::Done | codeterm_types::plan::TaskStatus::Cancelled
    ) {
        return Err(plan_rpc_err(PlanError::InvalidSplitParent {
            reason: codeterm_service_core::service::plan::SplitParentReason::TerminalParent,
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
        .authorize_draft_structure_mutation_typed(&plan_id, &authenticated_caller(p))
        .map_err(|error| {
            if error == codeterm_service_core::service::PlanAuthorityError::StoreReadFailed {
                super::super::plan_task_authority::planning_authority_error(error)
            } else {
                JsonRpcError {
                    code: -32003,
                    message: error.to_string(),
                    data: Some(json!({"type":"plan_structure_authorization_required"})),
                }
            }
        })?;
    if matches!(
        &author,
        codeterm_service_core::plan::PlanningAuthor::Planner { .. }
    ) {
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
        codeterm_service_core::service::ensure_planner_task_fields_are_structural(&author, child)
            .map_err(|message| JsonRpcError {
            code: -32003,
            message,
            data: Some(json!({"type":"plan_role_authorization_required"})),
        })?;
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
            .unwrap_or_else(|| json!(parent.acceptance_criteria));
        let sections = child
            .get("sections")
            .cloned()
            .unwrap_or_else(|| json!(parent.section_ids));
        let sections = match sections {
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","),
            Value::String(value) => value,
            _ => String::new(),
        };
        let mut child_params = json!({
            "plan_id": plan_id,
            "name": name,
            "parent": parent_id,
            "type": parent.kind,
            "body": string("body", parent.body.as_deref()).unwrap_or_default(),
            "acceptance_criteria": criteria,
            "test_plan": string("test_plan", parent.test_plan.as_deref()),
            "qa_plan": string("qa_plan", parent.qa_plan.as_deref()),
            "correctness": string("correctness", parent.correctness.as_deref()),
            "sections": sections,
            "executor_role": string("executor_role", parent.executor_role.as_deref()),
            "personality": string("personality", parent.personality.as_deref()),
            "role_guidance": string("role_guidance", parent.role_guidance.as_deref()),
            "model": string("model", parent.model.as_deref()),
            "why": string("model_rationale", parent.model_rationale.as_deref()),
            "intent": string("intent", parent.intent.as_deref()),
            "tags": child.get("tags"),
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
        let personality =
            codeterm_types::plan::require_executor_name(child_params["personality"].as_str())
                .map_err(|message| plan_rpc_err(PlanError::InvalidExecutorName(message)))?;
        child_params["personality"] = json!(personality);
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
            files: super::task_files_field(Some(child))?.unwrap_or_default(),
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
            tags: tags_field(Some(child)),
            ..Default::default()
        });
    }
    let ids = store
        .add_child_tasks_atomic(&plan_id, &parent_id, new_children, now)
        .map_err(plan_rpc_err)?;
    emit_plan_changed(svc, &plan_id);
    let created = ids
        .into_iter()
        .map(|id| {
            let (key, section_ids) = match store
                .task_creation_projection(&plan_id, &id)
                .map_err(plan_rpc_err)?
            {
                Some(read) => (Some(read.key), read.section_ids),
                None => (None, Vec::new()),
            };
            Ok::<_, JsonRpcError>(json!({"task_id":id,"key":key,"section_ids":section_ids}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"plan_id": plan_id, "parent_task_id": parent_id, "children": created}))
}
