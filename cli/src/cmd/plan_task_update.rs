use crate::client::RpcClient;
use crate::cmd::plan::{
    call_plan_rpc, inject_trusted_session, resolve_role_guidance, resolve_text,
};
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
    replace_criteria: bool,
    test_plan: Option<String>,
    qa_plan_text: Option<String>,
    correctness: Option<String>,
    parent: Option<String>,
    depends_on: Option<Vec<String>>,
    tab: Option<String>,
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
    external_refs: Vec<String>,
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
        if replace_criteria {
            params["acceptance_criteria_replace"] = json!(true);
        }
    }
    if let Some(t) = test_plan {
        params["test_plan"] = json!(t);
    }
    if let Some(plan) = qa_plan_text {
        params["qa_plan"] = json!(plan);
    }
    if let Some(c) = correctness {
        params["correctness"] = json!(c);
    }
    if let Some(p) = parent {
        params["parent_task_id"] = json!(p);
    }
    if let Some(deps) = depends_on {
        params["depends_on"] = json!(deps);
    }
    if let Some(lane) = crate::cmd::plan::lane_params(
        tab,
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
    if !external_refs.is_empty() {
        params["external_refs"] = json!(external_refs);
    }
    inject_trusted_session(&mut params);
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
    speed: Option<String>,
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
    replace_criteria: bool,
    test_plan: Option<String>,
    qa_plan: Option<String>,
    qa_plan_file: Option<String>,
    correctness: Option<String>,
    parent: Option<String>,
    depends_on: Vec<String>,
    remove_dependency: Vec<String>,
    replace_dependencies: bool,
    clear_dependencies: bool,
    tab: Option<String>,
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
    remove_tags: Vec<String>,
    primary: Option<String>,
    handle: Option<String>,
    external_refs: Vec<String>,
) -> Result<Value, String> {
    if let Some(personality) = personality.as_deref() {
        codeterm_types::plan::validate_executor_name(personality)?;
    }
    let why_text = resolve_text(why, why_file)?;
    let body_text = resolve_text(body, body_file)?;
    let role_guidance_text = resolve_role_guidance(role_guidance, role_guidance_file)?;
    let intent_text = resolve_text(intent, intent_file)?;
    let qa_plan_text = resolve_text(qa_plan, qa_plan_file)?;
    let resolved_depends_on = resolve_dependencies(
        client,
        &task_id,
        depends_on,
        remove_dependency,
        replace_dependencies,
        clear_dependencies,
    )?;
    let mut params = build_params(
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
        replace_criteria,
        test_plan,
        qa_plan_text,
        correctness,
        parent,
        resolved_depends_on,
        tab,
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
        external_refs,
    );
    if let Some(speed) = speed {
        params["speed"] = json!(speed);
    }
    if !remove_tags.is_empty() {
        params["remove_tags"] = json!(remove_tags);
    }
    call_plan_rpc(client, "plan.task.update", params)
}

fn dedup_preserve(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

fn merge_dependencies(existing: Vec<String>, add: &[String], remove: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dep in existing.into_iter().chain(add.iter().cloned()) {
        if remove.contains(&dep) || out.contains(&dep) {
            continue;
        }
        out.push(dep);
    }
    out
}

fn fetch_dependencies(client: &mut dyn RpcClient, task_id: &str) -> Result<Vec<String>, String> {
    let val = call_plan_rpc(client, "task.show", json!({ "task_id": task_id }))?;
    let deps = val
        .get("task")
        .unwrap_or(&val)
        .get("depends_on")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(deps)
}

fn resolve_dependencies(
    client: &mut dyn RpcClient,
    task_id: &str,
    depends_on: Vec<String>,
    remove: Vec<String>,
    replace: bool,
    clear: bool,
) -> Result<Option<Vec<String>>, String> {
    if clear {
        return Ok(Some(vec![]));
    }
    if replace {
        return Ok(Some(dedup_preserve(depends_on)));
    }
    if depends_on.is_empty() && remove.is_empty() {
        return Ok(None);
    }
    let existing = fetch_dependencies(client, task_id)?;
    Ok(Some(merge_dependencies(existing, &depends_on, &remove)))
}

#[cfg(test)]
mod tests;
