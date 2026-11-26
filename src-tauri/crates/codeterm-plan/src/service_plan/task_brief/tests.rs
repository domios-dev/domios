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

#[test]
fn dispatched_brief_compacts_strides_with_opaque_fragments_and_read_command() {
    let short_decision = "opaque-short-dec-71";
    let long_decision = format!(
        "opaque-long-dec-head-72 {} opaque-long-dec-tail-72",
        "seg ".repeat(60)
    );
    let long_report = format!(
        "opaque-report-head-73-{}opaque-report-tail-73",
        "x".repeat(260)
    );
    let task: Task = serde_json::from_value(serde_json::json!({
        "id": "task_brief_strides",
        "name": "opaque-brief-title",
        "type": "code",
        "status": "pending",
        "milestones": [
            {"id": "ms_noise", "from": {}, "kind": "status_update", "text": "opaque-noise-70", "ts": 1764000300000u64},
            {"id": "ms_short_dec", "from": {}, "kind": "decision", "text": short_decision, "ts": 1764000000000u64},
            {"id": "ms_long_dec", "from": {}, "kind": "decision", "text": long_decision, "ts": 1764000060000u64},
            {"id": "ms_report", "from": {}, "kind": "worker_report", "text": long_report, "ts": 1764000120000u64}
        ]
    }))
    .unwrap();

    let brief = render_task_brief(&task, &[], None, None, Some("pl_brief"), &[], &[]);
    assert!(brief.contains("<codeterm-task-strides task=\"task_brief_strides\" count=\"3\">"));
    assert!(brief.contains("</codeterm-task-strides>"));
    assert!(brief.contains(short_decision));
    assert!(brief.contains("opaque-long-dec-head-72"));
    assert!(!brief.contains("opaque-long-dec-tail-72"));
    assert!(brief.contains("ms_long_dec"));
    assert!(brief.contains("opaque-report-head-73-"));
    assert!(!brief.contains("opaque-report-tail-73"));
    assert!(!brief.contains("opaque-noise-70"));
    assert!(brief.contains("domios plan task show pl_brief task_brief_strides"));

    let tool_less = render_tool_less_task_brief(&task, &[], None, None);
    assert!(tool_less.contains("<codeterm-task-strides task=\"task_brief_strides\" count=\"3\">"));
    assert!(!tool_less.contains("domios plan task show"));
    assert!(!tool_less.contains("opaque-report-tail-73"));
}
