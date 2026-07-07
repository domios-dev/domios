use super::build_brief;
use crate::plan::Task;
use crate::service::plan::task_queries::TaskSection;
use serde_json::json;

fn task(value: serde_json::Value) -> Task {
    serde_json::from_value(value).expect("task fixture")
}

#[test]
fn brief_includes_body_acceptance_and_report_command() {
    let t = task(json!({
        "id": "task_1",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "body": "Compose spawn, assign, send.",
        "acceptance_criteria": ["atomic", "returns pane id"],
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.contains("# Wire dispatch RPC"));
    assert!(brief.contains("Compose spawn, assign, send."));
    assert!(brief.contains("- atomic"));
    assert!(brief.contains("- returns pane id"));
    // The report command is mandatory so the worker can report back.
    assert!(brief.contains("codeterm agent report"));
}

#[test]
fn brief_lists_section_links_and_test_plan() {
    let t = task(json!({
        "id": "task_2",
        "name": "T",
        "type": "code",
        "status": "pending",
        "test_plan": "cargo test dispatch",
    }));
    let sections = vec![TaskSection {
        id: "sec_9".into(),
        title: "Phase 1".into(),
    }];
    let brief = build_brief(&t, &sections);
    assert!(brief.contains("## Test plan"));
    assert!(brief.contains("cargo test dispatch"));
    assert!(brief.contains("## Sections"));
    assert!(brief.contains("Phase 1 (sec_9)"));
}

#[test]
fn brief_omits_empty_optional_sections() {
    let t = task(json!({
        "id": "task_3",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(!brief.contains("## Acceptance criteria"));
    assert!(!brief.contains("## Test plan"));
    assert!(!brief.contains("## Sections"));
    assert!(!brief.contains("## Strides"));
    assert!(brief.contains("codeterm agent report"));
}
