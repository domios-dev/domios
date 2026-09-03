use super::{agent_system_prompt, spawn_onboarding_task, worker_agent_system_prompt};
use crate::service::orchestrator::types::SpawnAgentParams;

fn orchestrator_params(task: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some("test-provider".into()),
        task: task.map(str::to_string),
        role: Some("Orchestrator".into()),
        ..Default::default()
    }
}

#[test]
fn plain_orchestrator_no_task_spawn_receives_the_system_contract() {
    let prompt =
        spawn_onboarding_task(&orchestrator_params(None)).expect("orchestrator system prompt");
    assert!(prompt.starts_with(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(prompt.contains(crate::prompts::orchestrator_suffix().trim()));
}

#[test]
fn explicit_orchestrator_task_is_submitted_exactly_once() {
    let prompt = spawn_onboarding_task(&orchestrator_params(Some("inspect launch")))
        .expect("orchestrator system prompt");
    assert_eq!(prompt.matches("inspect launch").count(), 1);
    assert_eq!(
        prompt
            .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
    );
}

#[test]
fn planner_onboarding_remains_work_when_a_task_is_provided() {
    let params = SpawnAgentParams {
        task: Some("plan this".into()),
        role: Some("Manager".into()),
        role_profile: Some("planner".into()),
        ..Default::default()
    };
    assert!(spawn_onboarding_task(&params)
        .unwrap()
        .contains(&crate::prompts::planner_suffix()));
}

/// Every launch composition, by capability class rather than provider id: a
/// spawned manager, a spawned Worker, each with and without an assignment.
fn every_launch_composition() -> Vec<(&'static str, String)> {
    vec![
        ("spawned non-worker", agent_system_prompt(None)),
        ("spawned worker", worker_agent_system_prompt(None, None)),
        (
            "spawned non-worker with assignment",
            agent_system_prompt(Some("do the thing")),
        ),
        (
            "spawned worker with assignment",
            worker_agent_system_prompt(Some("do the thing"), None),
        ),
        (
            "spawned orchestrator",
            spawn_onboarding_task(&orchestrator_params(Some("inspect launch")))
                .expect("orchestrator system prompt"),
        ),
        (
            "spawned planner",
            super::role_onboarding_task(Some("plan it"), Some("Manager"), Some("planner"))
                .expect("planner system prompt"),
        ),
        (
            "spawned watcher",
            super::role_onboarding_task(Some("watch it"), Some("Manager"), Some("watcher"))
                .expect("watcher system prompt"),
        ),
        // Capability classes, by declaration rather than provider id: a provider
        // that takes a system prompt, and one that must receive the same
        // composition through confirmed fallback.
        (
            "solo on a system-prompt-capable provider",
            super::role_launch_task(Some("do it"), None, None, true).expect("solo system prompt"),
        ),
        (
            "roled launch on a fallback-only provider",
            super::role_launch_task(Some("fix it"), Some("Worker"), None, false)
                .expect("fallback composition"),
        ),
    ]
}

/// The delivery half of the AgentPad invariant. The pre-existing prompt test
/// only forbade duplication, so a recipient that never received the rule was
/// invisible — which is exactly how spawned agents went without it.
#[test]
fn every_spawned_recipient_receives_the_agent_pad_rule_exactly_once() {
    let rule = crate::prompts::AGENT_PAD_RULE.trim();
    assert!(!rule.is_empty(), "an empty fragment proves nothing");

    for (recipient, composed) in every_launch_composition() {
        assert_eq!(
            composed.matches(rule).count(),
            1,
            "{recipient} must receive the AgentPad rule exactly once"
        );
    }
}

/// A Worker is the recipient that had neither the rule nor the pointer to the
/// skill carrying the full reference. Both arrive as whole constants.
#[test]
fn spawned_worker_receives_the_codeterm_primer() {
    let composed = worker_agent_system_prompt(Some("do the thing"), None);
    assert!(composed.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(composed.contains(crate::prompts::WORKER_CODETERM_CONTEXT.trim()));
}

/// A task narrows the doctrine its worker runs under through the existing
/// identity-overlay seam: `Task.role_guidance` is composed into an identity
/// block (`worker_identity::attach_worker_identity`), delimited by
/// `WORKER_IDENTITY_END`, then `worker_agent_system_prompt` splits it back out
/// and threads it through `persona::guidance_for` as the Worker preset's
/// `Task`-sourced fragment. This proves the round trip with an opaque
/// test-supplied token — never prompt wording — and that the transport marker
/// leaves no residue in the composed brief.
#[test]
fn per_task_doctrine_override_reaches_the_composed_worker_brief() {
    const TASK_DOCTRINE_TOKEN: &str = "OPAQUE-TASK-DOCTRINE-OVERRIDE-9f2c1";
    let identity = format!("You are the Worker. {TASK_DOCTRINE_TOKEN}");
    let brief =
        crate::service::plan::worker_identity::attach_worker_identity(&identity, "# Task body");

    let composed = super::role_onboarding_task(Some(&brief), Some("Worker"), None)
        .expect("worker system prompt");

    assert_eq!(composed.matches(TASK_DOCTRINE_TOKEN).count(), 1);
    assert!(!composed.contains(crate::service::plan::worker_identity::WORKER_IDENTITY_END));
}

/// Rich-rendering guidance is a delivery fact. Two tests in prompts.rs were named
/// for the primer and the worker context but asserted phrases inside
/// RICH_RENDERING_GUIDANCE itself, so they proved nothing about either recipient.
#[test]
fn every_spawned_recipient_receives_rich_rendering_guidance() {
    let guidance = crate::prompts::RICH_RENDERING_GUIDANCE.trim();

    for (recipient, composed) in [
        ("spawned non-worker", agent_system_prompt(None)),
        ("spawned worker", worker_agent_system_prompt(None, None)),
    ] {
        assert_eq!(
            composed.matches(guidance).count(),
            1,
            "{recipient} must receive the rich-rendering guidance exactly once"
        );
    }
}
