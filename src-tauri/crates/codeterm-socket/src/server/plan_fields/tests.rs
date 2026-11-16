use serde_json::json;

use super::execution_intent_field;
use codeterm_service_core::plan::TaskExecutionIntent;

#[test]
fn an_unclassified_task_mutates_the_repository_without_reading_the_project() {
    let intent = execution_intent_field(Some(&json!({}))).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::RepositoryMutation);
    let intent = execution_intent_field(None).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::RepositoryMutation);
}

#[test]
fn an_explicit_intent_is_kept() {
    let params = json!({ "execution_intent": "non_repository" });
    let intent = execution_intent_field(Some(&params)).expect("intent");
    assert_eq!(intent, TaskExecutionIntent::NonRepository);
}
