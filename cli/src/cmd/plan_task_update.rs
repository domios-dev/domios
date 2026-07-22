use crate::client::RpcClient;
use crate::cmd::plan::{call_plan_rpc, maybe_inject_caller_pane, resolve_text};
use serde_json::{json, Value};

#[allow(clippy::too_many_arguments)]
fn build_params(
    plan_id: Option<String>,
    task_id: String,
    name: Option<String>,
    kind: Option<String>,
    model: Option<String>,
    why_text: Option<String>,
    status: Option<String>,
    status_label: Option<String>,
    body_text: Option<String>,
    role_guidance_text: Option<String>,
    executor_role: Option<String>,
    personality: Option<String>,
    intent_text: Option<String>,
    sections: Option<String>,
    criterion: Vec<String>,
    test_plan: Option<String>,
    correctness: Option<String>,
    parent: Option<String>,
    depends_on: Vec<String>,
    clear_dependencies: bool,
    pane: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    worktree: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    destination: Option<String>,
    cleanup_status: Option<String>,
    cleanup_hints: Vec<String>,
    group: Option<String>,
    priority: Option<String>,
    tags: Vec<String>,
    primary: Option<String>,
    handle: Option<String>,
) -> Value {
    let mut params = json!({ "task_id": task_id });
    if let Some(pid) = plan_id {
        params["plan_id"] = json!(pid);
    }
    if let Some(n) = name {
        params["name"] = json!(n);
    }
    if let Some(k) = kind {
        params["type"] = json!(k);
    }
    if let Some(m) = model {
        params["model"] = json!(m);
    }
    if let Some(w) = why_text {
        params["why"] = json!(w);
    }
    if let Some(s) = status {
        params["status"] = json!(s);
    }
    if let Some(l) = status_label {
        params["status_label"] = json!(l);
    }
    if let Some(b) = body_text {
        params["body"] = json!(b);
    }
    if let Some(guidance) = role_guidance_text {
        params["role_guidance"] = json!(guidance);
    }
    if let Some(role) = executor_role {
        params["executor_role"] = json!(role);
    }
    if let Some(name) = personality {
        params["personality"] = json!(name);
    }
    if let Some(i) = intent_text {
        params["intent"] = json!(i);
    }
    if let Some(s) = sections {
        params["sections"] = json!(s);
    }
    if !criterion.is_empty() {
        params["acceptance_criteria"] = json!(criterion);
    }
    if let Some(t) = test_plan {
        params["test_plan"] = json!(t);
    }
    if let Some(c) = correctness {
        params["correctness"] = json!(c);
    }
    if let Some(p) = parent {
        params["parent_task_id"] = json!(p);
    }
    if clear_dependencies {
        params["depends_on"] = json!([]);
    } else if !depends_on.is_empty() {
        params["depends_on"] = json!(depends_on);
    }
    if let Some(lane) = crate::cmd::plan::lane_params(
        pane,
        workspace_id,
        workspace_name,
        worktree,
        branch,
        base,
        destination,
        cleanup_status,
        cleanup_hints,
    ) {
        params["lane"] = lane;
    }
    if let Some(g) = group {
        params["group"] = json!(g);
    }
    if let Some(priority) = priority {
        params["priority"] = json!(priority);
    }
    if !tags.is_empty() {
        params["tags"] = json!(tags);
    }
    if let Some(primary) = primary {
        params["primary"] = json!(primary);
    }
    if let Some(handle) = handle {
        params["handle"] = json!(handle);
    }
    maybe_inject_caller_pane(&mut params);
    params
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    client: &mut dyn RpcClient,
    plan_id: Option<String>,
    task_id: String,
    name: Option<String>,
    kind: Option<String>,
    model: Option<String>,
    why: Option<String>,
    why_file: Option<String>,
    status: Option<String>,
    status_label: Option<String>,
    body: Option<String>,
    body_file: Option<String>,
    role_guidance: Option<String>,
    role_guidance_file: Option<String>,
    executor_role: Option<String>,
    personality: Option<String>,
    intent: Option<String>,
    intent_file: Option<String>,
    sections: Option<String>,
    criterion: Vec<String>,
    test_plan: Option<String>,
    correctness: Option<String>,
    parent: Option<String>,
    depends_on: Vec<String>,
    clear_dependencies: bool,
    pane: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    worktree: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    destination: Option<String>,
    cleanup_status: Option<String>,
    cleanup_hints: Vec<String>,
    group: Option<String>,
    priority: Option<String>,
    tags: Vec<String>,
    primary: Option<String>,
    handle: Option<String>,
) -> Result<Value, String> {
    let why_text = resolve_text(why, why_file)?;
    let body_text = resolve_text(body, body_file)?;
    let role_guidance_text = resolve_text(role_guidance, role_guidance_file)?;
    let intent_text = resolve_text(intent, intent_file)?;
    let params = build_params(
        plan_id,
        task_id,
        name,
        kind,
        model,
        why_text,
        status,
        status_label,
        body_text,
        role_guidance_text,
        executor_role,
        personality,
        intent_text,
        sections,
        criterion,
        test_plan,
        correctness,
        parent,
        depends_on,
        clear_dependencies,
        pane,
        workspace_id,
        workspace_name,
        worktree,
        branch,
        base,
        destination,
        cleanup_status,
        cleanup_hints,
        group,
        priority,
        tags,
        primary,
        handle,
    );
    call_plan_rpc(client, "plan.task.update", params)
}

#[cfg(test)]
mod tests {
    use super::build_params;
    use serde_json::json;

    #[test]
    fn omitted_dependencies_leave_params_unchanged() {
        let params = build_params(
            Some("pl_1".into()),
            "task_1".into(),
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
            vec![],
            None,
            None,
            None,
            vec![],
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
        );
        assert!(params.get("depends_on").is_none());
    }

    #[test]
    fn depends_on_replaces_dependencies_in_params() {
        let params = build_params(
            Some("pl_1".into()),
            "task_1".into(),
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
            vec![],
            None,
            None,
            None,
            vec!["task_a".into(), "task_b".into()],
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
        );
        assert_eq!(
            params.get("depends_on").cloned(),
            Some(json!(["task_a", "task_b"]))
        );
    }

    #[test]
    fn clear_dependencies_emits_empty_array() {
        let params = build_params(
            Some("pl_1".into()),
            "task_1".into(),
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
            vec![],
            None,
            None,
            None,
            vec!["task_a".into()],
            true,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
        );
        assert_eq!(params.get("depends_on").cloned(), Some(json!([])));
    }

    #[test]
    fn status_and_pane_share_one_update_rpc_payload() {
        let params = build_params(
            Some("pl_1".into()),
            "task_1".into(),
            None,
            None,
            None,
            None,
            Some("in_progress".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            None,
            vec![],
            false,
            Some("worker-pane".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
        );
        assert_eq!(params.get("status"), Some(&json!("in_progress")));
        assert_eq!(params.pointer("/lane/pane_id"), Some(&json!("worker-pane")));
    }
}
