use super::*;
#[test]
fn tool_less_dispatch_omits_cli_lifecycle_fragments_and_preserves_task() {
    let task: Task = serde_json::from_value(serde_json::json!({"id":"task-opaque", "name":"opaque title", "type":"unplanned", "status":"pending", "body":"opaque assignment"})).unwrap();
    let command = codeterm_service_core::service::plan::task_attempts::task_report_command(
        "plan-opaque",
        &task.id,
    );
    let ordinary = build_brief_for_capabilities(false, (&task, &[][..]), "plan-opaque", &[], &[]);
    assert!(ordinary.contains(&command));
    let managed = build_brief_for_capabilities(true, (&task, &[][..]), "plan-opaque", &[], &[]);
    assert!(!managed.contains(&command));
    assert!(!managed
        .contains(codeterm_service_core::service::plan::worker_identity::WORKER_ACK_INSTRUCTION));
    assert_eq!(managed.matches("opaque assignment").count(), 1);
}

#[test]
fn sql_dispatch_brief_matches_the_held_contract_for_both_capabilities() {
    use codeterm_service_core::plan::{OrchestratorRef, Plan};
    use codeterm_service_core::service::plan::PlanStore;
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "brief parity".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    let task: Task=serde_json::from_value(serde_json::json!({"id":"selected", "name":"opaque name", "type":"unplanned", "tags":["unplanned"], "personality":"Ada", "status":"pending", "body":"opaque contract", "acceptance_criteria":["opaque criterion"]})).unwrap();
    plan.tasks = vec![task.clone()];
    store.save(&mut plan, 1).unwrap();
    let preparation = store
        .worker_task_preparation_projection(&plan.id, &task.id)
        .unwrap();
    let facts = store
        .task_dispatch_preflight_projection(&plan.id, &task.id)
        .unwrap();
    let gates = vec!["opaque gate".into()];
    for tool_less in [false, true] {
        assert_eq!(
            build_brief_for_capabilities(tool_less, (&preparation, &facts), &plan.id, &[], &gates),
            build_brief_for_capabilities(tool_less, (&task, &[][..]), &plan.id, &[], &gates)
        );
    }
}
