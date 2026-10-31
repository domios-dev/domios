use super::*;
use codeterm_service_core::plan::{OrchestratorRef, Plan, TagFacet};
use codeterm_service_core::service::plan::PlanStore;

#[test]
fn ease_task_add_aggregates_errors_and_derives_optional_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Draft".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tag_facets.push(TagFacet {
        key: "work".into(),
        label: None,
        color_lane: false,
        values: vec![],
    });
    store.save(&mut plan, 1).unwrap();
    let error = normalize(
        &Some(json!({"personality":"two words", "sections":"missing", "tags":["work:nope"]})),
        &store,
        &plan.id,
    )
    .unwrap_err();
    for field in [
        "--name",
        "--executor-role",
        "--role-guidance",
        "--personality",
        "--sections",
        "--tag",
    ] {
        assert!(error.message.contains(field), "{}", error.message);
    }
    assert!(store.get(&plan.id).unwrap().tasks.is_empty());
    let draft = normalize(&Some(json!({"name":"CLI defaults", "executor_role":"Engineer", "role_guidance":"Own the CLI", "model":"provider-fixture"})), &store, &plan.id).unwrap();
    assert_eq!(draft["personality"], "CLI");
    assert_eq!(draft["model"], "provider-fixture");
    assert_eq!(draft["tags"], json!(["work:backend"]));
    assert!(draft["inferred_work_warning"]
        .as_str()
        .unwrap()
        .contains("WARNING:"));
    assert!(draft.get("correctness").is_none());
}
