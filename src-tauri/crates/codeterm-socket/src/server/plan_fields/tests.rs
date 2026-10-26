use serde_json::json;

use super::execution_intent_field;
use codeterm_service_core::plan::TaskExecutionIntent;

#[test]
fn an_unclassified_task_outside_a_checkout_needs_no_repository() {
    let project = tempfile::tempdir().expect("project dir");
    let path = project.path().to_str().expect("utf-8 path");
    let intent = execution_intent_field(Some(&json!({})), Some(path)).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::NonRepository);
}

#[test]
fn an_unclassified_task_inside_a_checkout_mutates_the_repository() {
    let project = tempfile::tempdir().expect("project dir");
    std::fs::create_dir(project.path().join(".git")).expect("git marker");
    let nested = project.path().join("src");
    std::fs::create_dir(&nested).expect("nested dir");
    let intent = execution_intent_field(Some(&json!({})), nested.to_str()).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::RepositoryMutation);
}

#[test]
fn an_explicit_intent_wins_over_the_project_checkout() {
    let project = tempfile::tempdir().expect("project dir");
    let path = project.path().to_str().expect("utf-8 path");
    let params = json!({ "execution_intent": "repository_mutation" });
    let intent = execution_intent_field(Some(&params), Some(path)).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::RepositoryMutation);
}
