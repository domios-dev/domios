use super::{
    build_brief, build_brief_with_claimed_persona, dispatch_worker_identity,
    resolve_dispatch_routing,
};
use crate::plan::{IdentityProvenance, Task};
use crate::service::plan::task_queries::TaskSection;
use crate::service::SpawnIdentityProvenance;
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
fn brief_opens_with_role_guidance_before_title_and_body() {
    let t = task(json!({
        "id": "task_rg",
        "name": "Stabilize flicker",
        "type": "code",
        "status": "pending",
        "personality": "Shannon",
        "role_guidance": "You are a React/TypeScript perf engineer. RED LINE: assert the DOM end state.",
        "body": "Fix the bounce in the thread view.",
    }));
    let brief = build_brief(&t, &[]);
    let guidance = "You are Shannon. You are a React/TypeScript perf engineer. RED LINE: assert the DOM end state.";
    let guidance_pos = brief.find(guidance).expect("role guidance in brief");
    let title_pos = brief.find("# Stabilize flicker").expect("title in brief");
    let body_pos = brief
        .find("Fix the bounce in the thread view.")
        .expect("body in brief");
    assert_eq!(guidance_pos, 0);
    assert!(guidance_pos < title_pos);
    assert!(guidance_pos < body_pos);
}

#[test]
fn test_author_brief_carries_charter_reference_before_task() {
    let t = task(json!({
        "id": "task_test_author",
        "name": "Author spawn-readiness drill",
        "type": "test",
        "status": "pending",
        "executor_role": "test-author",
        "role_guidance": crate::prompts::TEST_AUTHOR_GUIDANCE,
        "body": "Write the drill that catches a readiness race.",
    }));
    let brief = build_brief(&t, &[]);
    let guidance_pos = brief
        .find(crate::prompts::TEST_AUTHOR_GUIDANCE)
        .expect("canonical test-author guidance in brief");
    let title_pos = brief
        .find("# Author spawn-readiness drill")
        .expect("title in brief");
    let body_pos = brief
        .find("Write the drill that catches a readiness race.")
        .expect("body in brief");

    assert_eq!(guidance_pos, 0);
    assert!(brief.contains("docs/contributing/test-authoring.md"));
    assert!(guidance_pos < title_pos);
    assert!(guidance_pos < body_pos);
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

#[test]
fn dispatch_preserves_only_explicit_user_personality() {
    let t = task(json!({
        "id": "task_user_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "personality_provenance": IdentityProvenance::UserSupplied,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::UserSupplied));
}

#[test]
fn dispatch_treats_planner_personality_as_provisional() {
    let t = task(json!({
        "id": "task_suggested_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Ada",
        "personality_provenance": IdentityProvenance::Suggested,
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Ada"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::Suggested));
}

#[test]
fn dispatch_treats_batch_personality_without_legacy_provenance_as_provisional() {
    let t = task(json!({
        "id": "task_batch_persona",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Lamport",
    }));
    let (title, provenance) = dispatch_worker_identity(&t);
    assert_eq!(title.as_deref(), Some("Lamport"));
    assert_eq!(provenance, Some(SpawnIdentityProvenance::Suggested));
}

#[test]
fn dispatch_brief_uses_atomically_claimed_persona_after_reroll() {
    let t = task(json!({
        "id": "task_batch_collision",
        "name": "Dispatch",
        "type": "code",
        "status": "pending",
        "personality": "Lamport",
        "personality_provenance": IdentityProvenance::Suggested,
        "role_guidance": "You are a concurrency engineer.",
    }));

    let brief = build_brief_with_claimed_persona(&t, &[], "Ada");

    assert!(brief.starts_with("You are Ada. You are a concurrency engineer."));
    assert!(!brief.contains("You are Lamport"));
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
