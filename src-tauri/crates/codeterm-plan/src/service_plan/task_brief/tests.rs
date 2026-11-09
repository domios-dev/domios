use super::*;

#[test]
fn tool_less_task_brief_preserves_task_and_instruction_without_cli_lifecycle_fragments() {
    let task: Task = serde_json::from_value(serde_json::json!({"id":"task-fixture", "name":"opaque title", "body":"opaque task payload", "type":"research", "status":"pending"})).unwrap();
    let brief = render_tool_less_task_brief(
        &task,
        &[],
        Some("opaque persona"),
        Some("opaque dispatch instruction"),
    );
    assert_eq!(brief.matches("opaque task payload").count(), 1);
    assert_eq!(brief.matches("opaque dispatch instruction").count(), 1);
    assert!(!brief.contains(super::super::worker_identity::WORKER_ACK_INSTRUCTION));
    assert!(brief.contains("opaque persona"));
}

#[test]
fn dispatched_worker_brief_requires_applicable_plan_skills_before_actions() {
    let manifest = vec![crate::plan::PlanSkillManifestEntry {
        slug: "dispatch-contract".into(),
        title: "Dispatch Contract".into(),
        purpose: "Use for Worker launches.".into(),
        content_digest: "sha256:abc".into(),
    }];
    let rendered = render_plan_skill_manifest("pl_zzmxgb", &manifest);
    assert!(rendered.contains("sha256:abc"));
    assert!(!rendered.contains("# Alpha"));
    assert!(render_plan_skill_manifest("pl_zzmxgb", &[]).is_empty());
}

#[test]
fn authorized_gate_tokens_round_trip_without_placeholder_residue() {
    let task: Task = serde_json::from_value(serde_json::json!({
        "id": "task_gates",
        "name": "gate transport",
        "type": "code",
        "status": "pending"
    }))
    .unwrap();
    let gates = vec!["gate-token-alpha".into(), "gate-token-beta".into()];
    let rendered = render_task_brief(&task, &[], None, None, None, &[], &gates);

    for gate in &gates {
        assert_eq!(rendered.matches(gate).count(), 1);
    }
    assert!(!rendered.contains("{authorized_gates}"));
}
