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
    assert_eq!(
        prompt
            .matches(crate::prompts::ORCH_CHILD_PTY_RECOVERY_TRIGGER)
            .count(),
        1
    );
}

#[test]
fn cartographer_prompt_orchestrator_composition_delivers_manager_notice() {
    let prompt =
        spawn_onboarding_task(&orchestrator_params(None)).expect("orchestrator system prompt");
    let notice = crate::prompts::ORCH_CARTOGRAPHER_CONTRACT_NOTICE;
    assert_eq!(prompt.matches(notice).count(), 1);
    assert!(!prompt.contains("{cartographer_contract_notice}"));
    assert!(prompt.len() <= crate::prompts::COMPOSED_ROLE_PROMPT_MAX_BYTES);
}

#[test]
fn cartographer_prompt_cartographer_composition_delivers_contract_and_assignment_once() {
    const ASSIGNMENT_TOKEN: &str = "OPAQUE-CARTOGRAPHER-DELIVERY-2c6e1";
    let prompt = super::role_onboarding_task(
        Some(ASSIGNMENT_TOKEN),
        Some("Manager"),
        Some("cartographer"),
    )
    .expect("cartographer system prompt");
    let contract = crate::prompts::CARTOGRAPHER_SUFFIX.trim();
    assert_eq!(prompt.matches(contract).count(), 1);
    assert_eq!(prompt.matches(ASSIGNMENT_TOKEN).count(), 1);
    assert!(!prompt.contains(crate::prompts::WORKER_SUFFIX.trim()));
    assert!(!prompt.contains(crate::prompts::planner_suffix().trim()));
    assert!(!prompt.contains("{cartographer_contract_notice}"));
    assert!(prompt.len() <= crate::prompts::COMPOSED_ROLE_PROMPT_MAX_BYTES);
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
        // Capability declarations determine composition, so provider identity need not select the prompt path.
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

/// A quoted coordination payload that survived the caller's shell only because
/// it was staged in a file still has to reach the receiver unchanged. Composition
/// is concatenation, never templating, so an opaque test-supplied payload holding
/// backticks, `$` expansions, command substitution, both quote kinds, a tab and
/// non-ASCII must appear once and verbatim in every recipient's brief.
#[test]
fn quoted_handoff_payload_reaches_every_recipient_brief_byte_exact() {
    const PAYLOAD: &str = "QUOTED-HANDOFF-7ac91 goal: keep `plan handoff` intact.\n\n\
                           - dollars: $CARGO_TARGET_DIR and ${HOME}\n\
                           - substitution: $(rm -rf /) and `whoami`\n\
                           - quotes: \"double\" and 'single'\n\
                           - tab:\tkept\n\
                           - unicode: Привіт — ok";

    let recipients = [
        (
            "spawned orchestrator",
            spawn_onboarding_task(&orchestrator_params(Some(PAYLOAD)))
                .expect("orchestrator system prompt"),
        ),
        (
            "spawned planner",
            super::role_onboarding_task(Some(PAYLOAD), Some("Manager"), Some("planner"))
                .expect("planner system prompt"),
        ),
        (
            "spawned worker",
            super::role_onboarding_task(Some(PAYLOAD), Some("Worker"), None)
                .expect("worker system prompt"),
        ),
    ];

    for (recipient, composed) in recipients {
        assert_eq!(
            composed.matches(PAYLOAD).count(),
            1,
            "{recipient} must receive the staged payload once, byte for byte"
        );
    }
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

/// A Worker staffed on a mesh peer owns no plan task — `plan task dispatch` is
/// local-only, so the peer receives `role: "Worker"` and nothing else binding.
/// The doctrine has to arrive on that shape alone: it carries the rule that
/// every turn ends in one `codeterm agent report`, which for a remote agent is
/// the difference between work landing and work sitting unread in a tab on
/// another machine. Identity is asserted by constant and the composed context
/// by an opaque test-supplied token, never by prompt wording.
#[test]
fn a_task_less_worker_shape_still_receives_the_worker_doctrine() {
    use codeterm_types::agent::role_kind::builtins;
    const ASSIGNMENT_TOKEN: &str = "OPAQUE-MESH-ASSIGNMENT-4b71e";

    let doctrine = crate::service::role_preset::builtin_guidance(builtins::WORKER)
        .expect("the builtin Worker kind carries bundled doctrine");
    let composed = super::role_onboarding_task(Some(ASSIGNMENT_TOKEN), Some("Worker"), None)
        .expect("worker system prompt");

    assert_eq!(
        composed.matches(ASSIGNMENT_TOKEN).count(),
        1,
        "the assignment reaches the composed context exactly once"
    );
    assert_eq!(
        composed.matches(doctrine.trim()).count(),
        1,
        "the Worker doctrine reaches the task-less shape exactly once"
    );
}

/// The roleless spawn is the fallback an operator reaches for when a shape is
/// refused, and it is the one that silently loses the doctrine. Pinning the
/// difference keeps `--role Worker` the answer rather than a habit.
#[test]
fn the_roleless_fallback_shape_carries_no_worker_doctrine() {
    use codeterm_types::agent::role_kind::builtins;
    const ASSIGNMENT_TOKEN: &str = "OPAQUE-ROLELESS-ASSIGNMENT-8d33a";

    let doctrine = crate::service::role_preset::builtin_guidance(builtins::WORKER)
        .expect("the builtin Worker kind carries bundled doctrine");
    let composed = super::role_onboarding_task(Some(ASSIGNMENT_TOKEN), None, None)
        .expect("roleless system prompt");

    assert_eq!(composed.matches(ASSIGNMENT_TOKEN).count(), 1);
    assert!(
        !composed.contains(doctrine.trim()),
        "a roleless spawn carries no Worker doctrine, which is why it is not the mesh shape"
    );
}

#[test]
fn mesh_spawn_return_route_reaches_the_launch_prompt_once() {
    const PEER: &str = "opaque-origin-peer-7da32";
    const TAB: &str = "opaque-origin-tab-b8219";
    const ASSIGNMENT: &str = "opaque-assignment-936ac";
    for (role, capable) in [
        (Some("Orchestrator"), true),
        (Some("Worker"), true),
        (None, false),
    ] {
        let params = SpawnAgentParams {
            role: role.map(str::to_string),
            task: Some(ASSIGNMENT.into()),
            report_target_mesh_peer: Some(PEER.into()),
            report_target_tab_id: Some(TAB.into()),
            ..Default::default()
        };
        let prompt = super::spawn_launch_task(&params, capable).expect("launch prompt");
        assert_eq!(prompt.matches(PEER).count(), 1);
        assert_eq!(prompt.matches(TAB).count(), 1);
        assert_eq!(prompt.matches(ASSIGNMENT).count(), 1);
    }
}

#[test]
fn mesh_spawn_return_route_does_not_guess_partial_or_local_targets() {
    for (peer, tab) in [
        (None, Some("opaque-local-tab")),
        (Some("opaque-peer"), None),
        (Some(" "), Some("opaque-tab")),
    ] {
        let params = SpawnAgentParams {
            task: Some("opaque-assignment".into()),
            report_target_mesh_peer: peer.map(str::to_string),
            report_target_tab_id: tab.map(str::to_string),
            ..Default::default()
        };
        assert_eq!(
            super::spawn_launch_task(&params, true),
            super::role_launch_task(params.task.as_deref(), None, None, true),
        );
    }
}

#[test]
fn mesh_spawn_return_route_prefers_the_matching_owned_peer_identity() {
    const PEER_NAME: &str = "opaque-peer-alias";
    const PEER_ID: &str = "opaque-stable-peer-id";
    let mut params = SpawnAgentParams {
        task: Some("opaque-assignment".into()),
        report_target_mesh_peer: Some(PEER_NAME.into()),
        report_target_tab_id: Some("opaque-recipient".into()),
        ownership: Some(codeterm_types::pty::RemoteOwnership {
            controller_title: "opaque-controller".into(),
            controller_tab_id: None,
            peer_id: PEER_ID.into(),
            peer_name: PEER_NAME.into(),
            team: None,
        }),
        ..Default::default()
    };
    let prompt = super::spawn_launch_task(&params, true).unwrap();
    assert_eq!(prompt.matches(PEER_ID).count(), 1);
    assert!(!prompt.contains(PEER_NAME));
    params.report_target_mesh_peer = Some("opaque-other-peer".into());
    let prompt = super::spawn_launch_task(&params, true).unwrap();
    assert!(prompt.contains("opaque-other-peer"));
    assert!(!prompt.contains(PEER_ID));
}

#[test]
fn tool_less_capability_selects_assignment_without_cli_contract_fragments() {
    let registry = crate::agent::registry::AgentRegistry::new();
    let task = crate::service::plan::worker_identity::attach_worker_identity(
        "opaque identity",
        "opaque assignment",
    );
    let params = SpawnAgentParams {
        agent: Some("opaque-provider".into()),
        role: Some("Worker".into()),
        task: Some(task),
        ..Default::default()
    };
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-provider").with_tool_less(true),
    ));
    assert_eq!(
        super::spawn_launch_task_for_provider(&registry, &params, false),
        Some("opaque identity\n\nopaque assignment".into())
    );
    assert_eq!(
        super::provider_launch_task(
            &registry,
            "opaque-provider",
            params.task.as_deref(),
            Some("Worker"),
            None
        ),
        Some("opaque identity\n\nopaque assignment".into())
    );
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "opaque-provider",
    )));
    let ordinary = super::spawn_launch_task_for_provider(&registry, &params, false).unwrap();
    assert!(ordinary.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(ordinary.contains(crate::service::plan::worker_identity::WORKER_ACK_INSTRUCTION));
}

#[test]
fn tool_less_composition_excludes_memory_management() {
    let rendered = super::tool_less_prompt(Some("OPAQUE_TOOL_LESS")).expect("assignment");
    assert_eq!(rendered, "OPAQUE_TOOL_LESS");
    assert_eq!(
        rendered
            .matches(crate::prompts::MEM_MANAGEMENT.trim())
            .count(),
        0
    );
}

#[test]
fn utility_guidance_reaches_managers_and_roleless_but_not_workers() {
    for (role, profile) in [
        (None, None),
        (Some("Orchestrator"), None),
        (Some("Manager"), Some("planner")),
    ] {
        let prompt = super::role_onboarding_task(Some("opaque-task"), role, profile).unwrap();
        assert_eq!(
            prompt
                .matches(crate::prompts::UTILITY_TAB_GUIDANCE.trim())
                .count(),
            1
        );
    }
    let worker = super::role_onboarding_task(Some("opaque-task"), Some("Worker"), None).unwrap();
    assert!(!worker.contains(crate::prompts::UTILITY_TAB_GUIDANCE.trim()));
    assert!(crate::prompts::UTILITY_TAB_GUIDANCE.lines().count() <= 3);
}

#[test]
fn built_in_plugins_section_follows_the_primer_and_is_absent_without_contributors() {
    const SECTION: &str = "Built-in plugins:\n- OPAQUE_CORE_LINE_q7";
    let primer = crate::prompts::AGENT_CODETERM_PRIMER.trim();
    let after_primer = format!("{primer}\n\n{SECTION}\n\n");

    let plain = super::agent_system_prompt_with_plugins(Some("opaque-task"), None);
    assert!(!plain.contains("Built-in plugins:"));
    let with = super::agent_system_prompt_with_plugins(Some("opaque-task"), Some(SECTION));
    assert!(with.starts_with(&after_primer));
    assert_eq!(with.replacen(&format!("{SECTION}\n\n"), "", 1), plain);

    let worker_plain = super::worker_agent_system_prompt_with_plugins(None, "opaque", None, None);
    assert!(!worker_plain.contains("Built-in plugins:"));
    let worker =
        super::worker_agent_system_prompt_with_plugins(None, "opaque", None, Some(SECTION));
    assert_eq!(worker.matches(&after_primer).count(), 1);
    assert_eq!(
        worker.replacen(&format!("{SECTION}\n\n"), "", 1),
        worker_plain
    );
}
