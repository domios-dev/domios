use super::*;
use crate::plan::types::{Plan, Section, Task, TaskStatus};

fn section(id: &str) -> Section {
    Section {
        id: id.into(),
        title: id.into(),
        handle: None,
        tags: vec![],
        body: String::new(),
        subsections: vec![],
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn task(sections: &[&str], status: TaskStatus) -> Task {
    Task {
        files: Default::default(),
        id: "t".into(),
        name: "n".into(),
        kind: "implement".into(),
        model: None,
        speed: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: sections.iter().map(|s| s.to_string()).collect(),
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: vec![],
        assigned_tab_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}

fn plan_with(sections: &[&str], tasks: &[(&[&str], TaskStatus)]) -> Plan {
    let mut p = Plan::new("T".into(), "/repo".into(), Default::default());
    p.sections = sections.iter().map(|s| section(s)).collect();
    p.tasks = tasks.iter().map(|(s, st)| task(s, st.clone())).collect();
    p
}

#[test]
fn coverage_counts_tasks_per_section() {
    let cov = compute_coverage(&plan_with(
        &["s1", "s2"],
        &[(&["s1"], TaskStatus::Done), (&["s1"], TaskStatus::Pending)],
    ));
    assert_eq!(cov.per_section["s1"].total, 2);
    assert_eq!(cov.per_section["s1"].done, 1);
    assert_eq!(cov.per_section["s2"].total, 0);
    assert!((cov.plan_covered_fraction - 0.5).abs() < 1e-9);
}

#[test]
fn report_lists_gaps_and_rolls_up_status() {
    let report = compute_coverage_report(&plan_with(
        &["s1", "s2"],
        &[(&["s1"], TaskStatus::Done), (&["s1"], TaskStatus::Pending)],
    ));
    assert_eq!(report.gaps, vec!["s2".to_string()]);
    assert_eq!(report.unlinked_count, 0);
    assert!((report.plan_covered_fraction - 0.5).abs() < 1e-9);
    let s1 = report.sections.iter().find(|s| s.title == "s1").unwrap();
    assert_eq!(s1.total, 2);
    assert_eq!(s1.done, 1);
    assert_eq!(s1.status, "in_progress");
    let s2 = report.sections.iter().find(|s| s.title == "s2").unwrap();
    assert_eq!(s2.status, "none");
}

#[test]
fn report_counts_unlinked_tasks() {
    let report = compute_coverage_report(&plan_with(
        &["s1"],
        &[(&[], TaskStatus::Pending), (&["s1"], TaskStatus::Done)],
    ));
    assert_eq!(report.unlinked_count, 1);
    assert_eq!(report.unlinked_tasks, vec!["n".to_string()]);
}
