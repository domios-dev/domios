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
    primary: Option<String>,
    handle: Option<String>,
    external_refs: Vec<String>,
) -> Result<Value, String> {
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
mod tests {
    use super::{build_params, dedup_preserve, merge_dependencies};
    use serde_json::json;

    #[allow(clippy::too_many_arguments)]
    fn params_with(
        criterion: Vec<String>,
        test_plan: Option<String>,
        qa_plan: Option<String>,
        status: Option<String>,
        depends_on: Option<Vec<String>>,
        tab: Option<String>,
        external_refs: Vec<String>,
    ) -> serde_json::Value {
        build_params(
            Some("pl_1".into()),
            "task_1".into(),
            None,
            None,
            None,
            None,
            status,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            criterion,
            false,
            test_plan,
            qa_plan,
            None,
            None,
            depends_on,
            tab,
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
            external_refs,
        )
    }

    fn params_with_model(model: Option<String>) -> serde_json::Value {
        build_params(
            Some("pl_1".into()),
            "task_1".into(),
            None,
            None,
            model,
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
            false,
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
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
            vec![],
        )
    }

    // `--model` is a free-form routing string stored verbatim; resolution to a
    // concrete provider model happens later at dispatch, not here.
    #[test]
    fn model_routing_string_is_stored_verbatim() {
        assert_eq!(
            params_with_model(Some("claude:haiku".into())).get("model"),
            Some(&json!("claude:haiku"))
        );
        assert_eq!(
            params_with_model(Some("claude".into())).get("model"),
            Some(&json!("claude"))
        );
        assert!(params_with_model(None).get("model").is_none());
    }

    #[test]
    fn external_ref_is_added_while_omitted_dependencies_stay_unchanged() {
        let params = params_with(
            vec![],
            None,
            None,
            None,
            None,
            None,
            vec!["rollacode/codeterm-releases#17".into()],
        );
        assert!(params.get("depends_on").is_none());
        assert_eq!(
            params.get("external_refs"),
            Some(&json!(["rollacode/codeterm-releases#17"]))
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn criteria_params(criterion: Vec<String>, replace_criteria: bool) -> serde_json::Value {
        build_params(
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
            criterion,
            replace_criteria,
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
            None,
            vec![],
            None,
            None,
            vec![],
            None,
            None,
            vec![],
        )
    }

    /// codeterm#372: no replace signal on the wire by default.
    #[test]
    fn criterion_defaults_to_append_with_no_replace_signal_on_the_wire() {
        let params = criteria_params(vec!["new criterion".into()], false);

        assert_eq!(
            params.get("acceptance_criteria"),
            Some(&json!(["new criterion"]))
        );
        assert!(
            params.get("acceptance_criteria_replace").is_none(),
            "append must not carry a replace flag: {params}"
        );
    }

    /// The daemon keys the destructive replace on this flag's presence.
    #[test]
    fn replace_criteria_flag_marks_the_update_as_destructive_on_the_wire() {
        let params = criteria_params(vec!["replacement".into()], true);

        assert_eq!(
            params.get("acceptance_criteria"),
            Some(&json!(["replacement"]))
        );
        assert_eq!(params.get("acceptance_criteria_replace"), Some(&json!(true)));
    }

    /// The bare flag alone never implicitly clears the array.
    #[test]
    fn replace_criteria_flag_alone_with_no_criterion_sends_no_acceptance_criteria() {
        let params = criteria_params(vec![], true);

        assert!(params.get("acceptance_criteria").is_none());
        assert!(params.get("acceptance_criteria_replace").is_none());
    }

    #[test]
    fn resolved_dependency_set_is_written_verbatim() {
        let params = params_with(
            vec![],
            None,
            None,
            None,
            Some(vec!["task_a".into(), "task_b".into()]),
            None,
            vec![],
        );
        assert_eq!(
            params.get("depends_on").cloned(),
            Some(json!(["task_a", "task_b"]))
        );
    }

    #[test]
    fn empty_resolved_dependency_set_clears_edges() {
        let params = params_with(vec![], None, None, None, Some(vec![]), None, vec![]);
        assert_eq!(params.get("depends_on").cloned(), Some(json!([])));
    }

    #[test]
    fn status_and_tab_share_one_update_rpc_payload() {
        let params = params_with(
            vec![],
            None,
            None,
            Some("in_progress".into()),
            None,
            Some("worker-tab".into()),
            vec![],
        );
        assert_eq!(params.get("status"), Some(&json!("in_progress")));
        assert_eq!(params.pointer("/lane/tab_id"), Some(&json!("worker-tab")));
    }

    #[test]
    fn qa_plan_is_independent_from_worker_test_plan() {
        let params = params_with(
            vec![],
            Some("cargo test worker_oracle".into()),
            Some("cargo test qa_oracle".into()),
            None,
            None,
            None,
            vec![],
        );
        assert_eq!(
            params.get("test_plan"),
            Some(&json!("cargo test worker_oracle"))
        );
        assert_eq!(params.get("qa_plan"), Some(&json!("cargo test qa_oracle")));
    }

    #[test]
    fn appending_one_dependency_preserves_the_existing_edges() {
        // Issue #275: a single --depends-on must append, never silently drop the rest.
        let existing: Vec<String> = [
            "task_x74mh8",
            "task_qsyimg",
            "task_12h0zm",
            "task_r2ecmw",
            "task_0qw4y4",
            "task_k0ke0q",
            "task_45lbsq",
            "task_mp2dfm",
            "task_y47ac0",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let merged = merge_dependencies(existing.clone(), &["task_yrggcs".into()], &[]);
        assert_eq!(merged.len(), 10, "one added, nine kept: {merged:?}");
        for edge in &existing {
            assert!(merged.contains(edge), "dropped existing edge {edge}");
        }
        assert!(merged.contains(&"task_yrggcs".to_string()));
    }

    #[test]
    fn merge_dependencies_dedups_additions_and_honors_removals() {
        let merged = merge_dependencies(
            vec!["task_a".into(), "task_b".into()],
            &["task_b".into(), "task_c".into()],
            &["task_a".into()],
        );
        assert_eq!(merged, vec!["task_b", "task_c"]);
    }

    #[test]
    fn dedup_preserve_keeps_first_occurrence_order() {
        assert_eq!(
            dedup_preserve(vec!["task_a".into(), "task_b".into(), "task_a".into()]),
            vec!["task_a", "task_b"]
        );
    }
}
