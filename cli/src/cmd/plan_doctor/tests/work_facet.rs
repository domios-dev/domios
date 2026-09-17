use super::super::*;
use super::write_v2_plan;

fn compliant_plan() -> Value {
    json!({
        "id": "pl_work", "title": "Work", "status": "active", "mode": "approved",
        "goal": {"vector": "persist explicit work tags"}, "primary_spec_id": "sp_work",
        "tag_facets": [{"key": "work", "values": []}, {"key": "area", "values": []}],
        "sections": [{"id": "sec_work", "title": "Work", "body": "Scoped task contract"}],
        "tasks": [{
            "id": "task_good", "name": "Good", "status": "pending",
            "section_ids": ["sec_work"], "body": "Enforce explicit work tagging",
            "acceptance_criteria": ["new tasks have one work value"],
            "test_plan": "Run `./build.ps1 test-one plan work_facet`; assert invalid drafts never persist. Then run `codeterm plan doctor pl_work --kind quality` and require zero findings for the compliant fixture.",
            "correctness": "preserves unrelated tags", "executor_role": "plan-contract-engineer",
            "tags": ["area:plan", "work:backend", "topic", "work"]
        }]
    })
}

#[test]
fn work_facet_quality_names_every_legacy_task_and_preserves_storage() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_work");
    let mut plan = compliant_plan();
    let cases = [
        ("task_missing", json!([]), "missing"),
        ("task_duplicate", json!(["work:qa", "work:qa"]), "multiple"),
        ("task_multiple", json!(["work:qa", "work:docs"]), "multiple"),
        ("task_invalid", json!(["work:unknown"]), "invalid"),
        ("task_empty", json!(["work:"]), "malformed"),
        ("task_nested", json!(["work:qa:docs"]), "malformed"),
        ("task_space", json!([" work:qa"]), "malformed"),
        ("task_value_space", json!(["work: qa"]), "malformed"),
    ];
    for (id, tags, _) in &cases {
        let mut task = plan["tasks"][0].clone();
        task["id"] = json!(id);
        task["tags"] = tags.clone();
        plan["tasks"].as_array_mut().unwrap().push(task);
    }
    write_v2_plan(&plan_dir, &plan);
    let before: Vec<_> = cases
        .iter()
        .map(|(id, _, _)| std::fs::read(plan_dir.join(format!("tasks/{id}.yaml"))).unwrap())
        .collect();
    let (report, calls) =
        super::authoritative_report_with_calls(&plan, "pl_work", dir.path(), "quality");
    assert!(
        calls.iter().all(|m| m == "plan.get" || m == "plan.files"),
        "the doctor must only read: {calls:?}"
    );
    assert_eq!(report["ok"], false);
    let findings = report["findings"].as_array().unwrap();
    assert_eq!(findings.len(), cases.len(), "{report}");
    let rendered = render_doctor(&report);
    for (index, (id, _, reason)) in cases.iter().enumerate() {
        let finding = findings
            .iter()
            .find(|finding| finding["task_id"] == *id)
            .unwrap();
        assert_eq!(finding["kind"], "work_facet_violation");
        assert_eq!(finding["severity"], "error");
        assert!(finding["detail"].as_str().unwrap().contains(*reason));
        assert!(!finding["fix"].as_str().unwrap().is_empty());
        assert!(rendered.contains(&format!("task={id}")));
        assert_eq!(
            std::fs::read(plan_dir.join(format!("tasks/{id}.yaml"))).unwrap(),
            before[index]
        );
    }
}

#[test]
fn work_facet_quality_leaves_compliant_plan_clean() {
    let plan = compliant_plan();
    let findings = inspect_plan(Path::new("pl_work.json"), &plan, "quality");
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn work_facet_quality_does_not_apply_to_undeclared_plans_or_storage_audit() {
    let mut plan = compliant_plan();
    plan["tasks"][0]["tags"] = json!(["work:", "work:custom", "work:custom", "topic"]);
    let storage = inspect_plan(Path::new("pl_work.json"), &plan, "storage");
    assert!(!storage
        .iter()
        .any(|finding| finding.kind == "work_facet_violation"));
    plan["tag_facets"] = json!([{"key": "area", "values": []}]);
    let findings = inspect_plan(Path::new("pl_work.json"), &plan, "quality");
    assert!(findings.is_empty(), "{findings:?}");
}
