use super::*;

fn review_task_ref(plan_id: &str, task_id: &str) -> String {
    format!("{plan_id}/{task_id}")
}

pub(super) fn is_qa(svc: &ServiceContext, tab_id: &str) -> bool {
    svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        codeterm_types::agent::role_kind::can_close_review(tab.role, tab.role_profile.as_deref())
    })
}

fn qa_review_actor(
    svc: &ServiceContext,
    plan_id: &str,
    task_id: &str,
    params: Option<&Value>,
) -> Result<AgentRef, JsonRpcError> {
    let caller = structural_caller(params);
    let tab_id = caller
        .tab_id
        .as_deref()
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| unauthorized("qa review requires its live tab binding"))?;
    let requested_session = caller
        .session_id
        .as_deref()
        .filter(|session| !session.trim().is_empty());
    let nonce = caller
        .binding_nonce
        .as_deref()
        .filter(|nonce| !nonce.trim().is_empty());
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or_else(|| unauthorized("the reviewed Plan no longer exists"))?;
    let owner_tab_id = plan
        .owner
        .tab_id
        .as_deref()
        .ok_or_else(|| unauthorized("the reviewed Plan has no live owner tab"))?;
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| unauthorized("the qa tab is no longer live"))?;
    let current_task_ref = review_task_ref(plan_id, task_id);
    let session_id = tab.agent_session_id.as_deref();
    let binding_nonce = tab.session_binding_nonce.as_deref();
    let attested = requested_session.is_none_or(|requested| Some(requested) == session_id)
        && session_id.is_some()
        && session_id == requested_session
        && nonce.is_some()
        && binding_nonce == nonce
        && tab.reported_session_binding_nonce.as_deref() == nonce;
    if !codeterm_types::agent::role_kind::can_close_review(tab.role, tab.role_profile.as_deref())
        || tab.task_binding.is_some()
        || tab.parent_tab_id.as_deref() != Some(owner_tab_id)
        || !tab
            .taskless_authorized_review_tasks
            .iter()
            .any(|grant| grant == &current_task_ref)
        || !attested
    {
        return Err(unauthorized(
            "the live QA was not handed this in-review task by its Plan owner",
        ));
    }
    Ok(AgentRef {
        tab_id: Some(tab.id.clone()),
        session_id: session_id.map(str::to_string),
        title: Some(tab.title.clone()),
    })
}

pub(super) fn authorized_review_actor(
    svc: &ServiceContext,
    plan_id: &str,
    task_id: &str,
    params: Option<&Value>,
) -> Result<AgentRef, JsonRpcError> {
    let caller = structural_caller(params);
    let candidate_tab_id = caller.tab_id.clone().or_else(|| actor(params).tab_id);
    let is_qa = candidate_tab_id
        .as_deref()
        .is_some_and(|tab_id| is_qa(svc, tab_id));
    if is_qa {
        return qa_review_actor(svc, plan_id, task_id, params);
    }
    if caller.tab_id.is_none()
        && caller.session_id.is_none()
        && caller.binding_nonce.is_none()
        && actor(params).tab_id.is_none()
    {
        return Ok(actor(params));
    }
    authorized_orchestrator_actor(svc, plan_id, params)
}

pub(super) fn delegate_review_task(
    svc: &ServiceContext,
    plan_id: &str,
    task_id: &str,
    qa_tab_id: &str,
    params: Option<&Value>,
) -> Result<Value, JsonRpcError> {
    let owner = authorized_orchestrator_actor(svc, plan_id, params)?;
    let owner_tab_id = owner
        .tab_id
        .as_deref()
        .ok_or_else(|| unauthorized("review delegation requires the live Plan owner tab"))?;
    let plan = svc
        .plan_store
        .get(plan_id)
        .ok_or_else(|| unauthorized("the reviewed Plan no longer exists"))?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| JsonRpcError {
            code: -32004,
            message: "review task not found in this Plan".into(),
            data: Some(json!({ "type": "review_task_not_found" })),
        })?;
    if task.status != codeterm_service_core::plan::TaskStatus::InReview {
        return Err(JsonRpcError {
            code: -32004,
            message: "only an in-review task can be handed to a QA".into(),
            data: Some(json!({ "type": "review_task_not_in_review" })),
        });
    }
    let task_ref = review_task_ref(plan_id, task_id);
    let mut mux = svc.mux.write();
    let qa = mux
        .get_tab(qa_tab_id)
        .ok_or_else(|| unauthorized("the QA tab is no longer live"))?;
    if !codeterm_types::agent::role_kind::can_close_review(qa.role, qa.role_profile.as_deref())
        || qa.task_binding.is_some()
        || qa.parent_tab_id.as_deref() != Some(owner_tab_id)
    {
        return Err(unauthorized(
            "review delegation requires a task-less QA that is a direct child of the Plan owner",
        ));
    }
    if mux.all_tabs().any(|tab| {
        tab.id != qa_tab_id
            && tab
                .taskless_authorized_review_tasks
                .iter()
                .any(|grant| grant == &task_ref)
    }) {
        return Err(JsonRpcError {
            code: -32004,
            message: "this review task is already handed to another QA".into(),
            data: Some(json!({ "type": "review_task_already_delegated" })),
        });
    }
    let qa = mux
        .get_tab_mut(qa_tab_id)
        .expect("the live QA remains in the mux write lock");
    if qa
        .taskless_authorized_review_tasks
        .iter()
        .any(|grant| grant == &task_ref)
    {
        return Err(JsonRpcError {
            code: -32004,
            message: "this review task is already handed to this QA".into(),
            data: Some(json!({ "type": "review_task_already_delegated" })),
        });
    }
    qa.taskless_authorized_review_tasks.push(task_ref);
    Ok(json!({ "plan_id": plan_id, "task_id": task_id, "qa_tab_id": qa_tab_id, "delegated": true }))
}

pub(super) fn consume_qa_review_grant(
    svc: &ServiceContext,
    from: &AgentRef,
    plan_id: &str,
    task_id: &str,
) {
    let Some(tab_id) = from.tab_id.as_deref() else {
        return;
    };
    if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
        let target = review_task_ref(plan_id, task_id);
        tab.taskless_authorized_review_tasks
            .retain(|grant| grant != &target);
    }
}
