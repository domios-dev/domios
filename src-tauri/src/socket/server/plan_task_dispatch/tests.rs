use super::{build_brief, resolve_dispatch_routing};
use crate::plan::Task;
use crate::service::plan::task_queries::TaskSection;
use serde_json::json;

fn providers() -> Vec<String> {
    vec!["claude".into(), "codex".into(), "opencode".into()]
}

#[test]
fn routing_explicit_agent_wins_and_keeps_model() {
    let r = resolve_dispatch_routing(
        Some("codex".into()),
        Some("gpt-5".into()),
        None,
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model.as_deref(), Some("gpt-5"));
}

#[test]
fn routing_model_naming_a_provider_becomes_that_provider() {
    // This is the exact BUG A repro: `--model codex` must route to the codex
    // provider at its default model, not to the default provider.
    let r = resolve_dispatch_routing(None, Some("codex".into()), None, &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_unknown_model_stays_a_model_for_default_provider() {
    let r = resolve_dispatch_routing(None, Some("opus".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model.as_deref(), Some("opus"));
}

#[test]
fn routing_falls_back_to_task_model_when_no_rpc_model() {
    let r = resolve_dispatch_routing(None, None, Some("codex".into()), &providers());
    assert_eq!(r.agent.as_deref(), Some("codex"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_rpc_model_overrides_task_model() {
    let r = resolve_dispatch_routing(
        None,
        Some("opencode".into()),
        Some("codex".into()),
        &providers(),
    );
    assert_eq!(r.agent.as_deref(), Some("opencode"));
    assert_eq!(r.model, None);
}

#[test]
fn routing_empty_inputs_use_defaults() {
    let r = resolve_dispatch_routing(Some("  ".into()), Some("".into()), None, &providers());
    assert_eq!(r.agent, None);
    assert_eq!(r.model, None);
}

#[test]
fn brief_opens_with_personality_line_when_set() {
    let t = task(json!({
        "id": "task_p",
        "name": "Wire dispatch RPC",
        "type": "code",
        "status": "pending",
        "personality": "Hopper",
    }));
    let brief = build_brief(&t, &[]);
    assert!(brief.starts_with("You are Hopper — executor for: Wire dispatch RPC"));
    assert!(brief.contains("# Wire dispatch RPC"));
}

#[test]
fn brief_omits_personality_line_when_absent() {
    let t = task(json!({
        "id": "task_np",
        "name": "Bare",
        "type": "code",
        "status": "pending",
    }));
    let brief = build_brief(&t, &[]);
    assert!(!brief.contains("You are"));
    assert!(brief.starts_with("# Bare"));
}

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
