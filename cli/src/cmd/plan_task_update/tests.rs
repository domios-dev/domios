use super::{build_params, merge_dependencies};
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
    assert_eq!(
        params.get("acceptance_criteria_replace"),
        Some(&json!(true))
    );
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
