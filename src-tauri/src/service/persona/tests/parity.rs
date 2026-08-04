//! Byte parity against an independent pre-image of the pre-table composition.
//! The pre-image is written out here on purpose: comparing the new path to
//! itself would prove nothing.

use codeterm_types::agent::role_kind::builtins;

use crate::multiplexer::types::AgentRole;
use crate::service::orchestrator::role_onboarding_task;
use crate::service::persona::{deliver, guidance_for, Occasion};

/// The launch assignment decoration as it read before the occasion table
/// existed, applied to the kind Task 1 resolves rather than the raw stored
/// profile. `pre_table_raw_profile_assignment` keeps the raw reading so the one
/// pair where the two disagree stays pinned instead of silently absorbed.
fn legacy_role_assignment_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let resolved = codeterm_types::agent::restore_role(role, role_profile);
    let role = resolved.as_ref().map(|resolved| resolved.role);
    let kind = codeterm_types::agent::role_kind::kind_id(
        role,
        resolved
            .as_ref()
            .and_then(|resolved| resolved.role_profile.as_deref()),
    );
    if kind == Some(builtins::ORCHESTRATOR) {
        let suffix = crate::prompts::orchestrator_suffix();
        return Some(match task {
            Some(task) => format!("{task}\n\n{suffix}"),
            None => suffix,
        });
    }
    if kind == Some(builtins::PLANNER) {
        return task.map(|t| format!("{}{}", t, crate::prompts::planner_suffix()));
    }
    if role == Some(AgentRole::Worker) {
        return task.map(|t| format!("{}{}", t, crate::prompts::worker_suffix()));
    }
    task.map(str::to_string)
}

/// Byte-for-byte the base branch chain, reading the raw stored profile. A
/// Worker naming a manager kind borrowed that manager's charter here.
fn pre_table_raw_profile_assignment(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let resolved = codeterm_types::agent::restore_role(role, role_profile);
    let role = resolved.as_ref().map(|resolved| resolved.role);
    let kind = resolved
        .as_ref()
        .and_then(|resolved| resolved.role_profile.as_deref());
    if kind == Some(builtins::ORCHESTRATOR) {
        let suffix = crate::prompts::orchestrator_suffix();
        return Some(match task {
            Some(task) => format!("{task}\n\n{suffix}"),
            None => suffix,
        });
    }
    if kind == Some(builtins::PLANNER) {
        return task.map(|t| format!("{}{}", t, crate::prompts::planner_suffix()));
    }
    if role == Some(AgentRole::Worker) {
        return task.map(|t| format!("{}{}", t, crate::prompts::worker_suffix()));
    }
    task.map(str::to_string)
}

fn legacy_agent_system_prompt(assignment: Option<&str>) -> String {
    let mut sections = vec![
        crate::prompts::AGENT_CODETERM_PRIMER.trim().to_string(),
        crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string(),
    ];
    if let Some(assignment) = assignment.filter(|text| !text.trim().is_empty()) {
        sections.push(format!("## Assignment\n\n{assignment}"));
    }
    sections.join("\n\n")
}

fn legacy_worker_system_prompt(assignment: Option<&str>) -> String {
    let (identity, assignment) = assignment
        .map(crate::service::plan::worker_identity::split_worker_identity)
        .unwrap_or((None, ""));
    let mut sections = vec![identity.unwrap_or("You are a CodeTerm Worker.").to_string()];
    sections.push(crate::prompts::WORKER_CODETERM_CONTEXT.trim().to_string());
    sections.push(crate::prompts::RICH_RENDERING_GUIDANCE.trim().to_string());
    if !assignment.trim().is_empty() {
        sections.push(format!("## Assignment\n\n{}", assignment.trim()));
    }
    sections.join("\n\n")
}

fn legacy_role_onboarding_task(
    task: Option<&str>,
    role: Option<&str>,
    role_profile: Option<&str>,
) -> Option<String> {
    let assignment = legacy_role_assignment_task(task, role, role_profile);
    if role == Some("Worker") {
        Some(legacy_worker_system_prompt(assignment.as_deref()))
    } else {
        Some(legacy_agent_system_prompt(assignment.as_deref()))
    }
}

fn legacy_manager_guidance(kind_id: &str) -> String {
    match kind_id {
        builtins::ORCHESTRATOR => crate::prompts::orchestrator_suffix(),
        builtins::PLANNER | builtins::WATCHER => crate::prompts::planner_suffix(),
        builtins::WORKER => crate::prompts::worker_suffix(),
        _ => crate::prompts::planner_suffix(),
    }
}

const ROLE_PAIRS: &[(Option<&str>, Option<&str>)] = &[
    (None, None),
    (None, Some(builtins::PLANNER)),
    (Some("Worker"), None),
    (Some("Worker"), Some("rust-platform-dev")),
    (Some("Worker"), Some(builtins::ORCHESTRATOR)),
    (Some("Worker"), Some(builtins::PLANNER)),
    (Some("Worker"), Some("  ")),
    (Some("Manager"), None),
    (Some("Manager"), Some(builtins::ORCHESTRATOR)),
    (Some("Manager"), Some(builtins::PLANNER)),
    (Some("Manager"), Some(builtins::WATCHER)),
    (Some("Manager"), Some(builtins::GENERAL_AGENT)),
    (Some("Manager"), Some(builtins::WORKER)),
    (Some("Manager"), Some("field-marshal")),
    (Some("Orchestrator"), None),
    (Some("Conductor"), Some(builtins::PLANNER)),
    (Some("bogus"), Some(builtins::PLANNER)),
];

fn worker_brief() -> String {
    crate::service::plan::worker_identity::attach_worker_identity(
        "You are Mira. Extend the shared preset seam.\nExecutor role: rust-backend-dev.",
        "# Extend shared role presets\n\nBody text.",
    )
}

fn payloads() -> Vec<Option<String>> {
    vec![
        None,
        Some(String::new()),
        Some("inspect launch".to_string()),
        Some(worker_brief()),
    ]
}

#[test]
fn launch_onboarding_output_is_byte_identical_to_the_pre_table_path() {
    for (role, role_profile) in ROLE_PAIRS {
        for payload in payloads() {
            let task = payload.as_deref();
            assert_eq!(
                role_onboarding_task(task, *role, *role_profile),
                legacy_role_onboarding_task(task, *role, *role_profile),
                "role={role:?} profile={role_profile:?} task={task:?}"
            );
        }
    }
}

#[test]
fn manager_bootstrap_guidance_is_byte_identical_to_the_pre_table_path() {
    for kind in [
        builtins::ORCHESTRATOR,
        builtins::PLANNER,
        builtins::WATCHER,
        builtins::WORKER,
        builtins::GENERAL_AGENT,
        "field-marshal",
        "",
        "   ",
    ] {
        assert_eq!(
            guidance_for(
                Occasion::ManagerBootstrap,
                Some(AgentRole::Manager),
                Some(kind),
                None
            ),
            legacy_manager_guidance(kind),
            "kind={kind:?}"
        );
    }
}

#[test]
fn the_worker_identity_slot_keeps_its_exact_default_and_persona() {
    assert_eq!(
        guidance_for(
            Occasion::WorkerLaunchIdentity,
            Some(AgentRole::Worker),
            None,
            None
        ),
        "You are a CodeTerm Worker."
    );
    assert_eq!(
        guidance_for(
            Occasion::WorkerLaunchIdentity,
            Some(AgentRole::Worker),
            Some("rust-backend-dev"),
            Some("You are Mira.")
        ),
        "You are Mira.",
        "the task persona is the only text in the identity slot"
    );
}

/// The one pair where the table deliberately does not reproduce the base bytes:
/// a Worker whose stored profile names a manager kind. Both halves are pinned.
#[test]
fn a_worker_naming_a_manager_kind_consumes_the_worker_contract_not_the_managers() {
    for (profile, borrowed) in [
        (builtins::ORCHESTRATOR, crate::prompts::orchestrator_suffix()),
        (builtins::PLANNER, crate::prompts::planner_suffix()),
    ] {
        // What the base shipped: the Worker was handed the manager's charter.
        let leaked = pre_table_raw_profile_assignment(Some("fix it"), Some("Worker"), Some(profile))
            .expect("pre-table assignment");
        assert!(
            leaked.contains(&borrowed),
            "the pre-table reading is expected to leak {profile} text"
        );

        // What ships now: Task 1 collapses the kind, so the Worker contract is
        // what the assignment occasion consumes.
        let assignment = deliver(
            Occasion::LaunchAssignment,
            Some(AgentRole::Worker),
            Some(profile),
            None,
            Some("fix it"),
        )
        .expect("worker assignment");
        assert_eq!(
            assignment,
            format!("fix it{}", crate::prompts::worker_suffix()),
            "a collapsed Worker kind must consume the Worker contract"
        );
        assert!(!assignment.contains(&borrowed));

        // The collapse never invents an assignment a plain Worker would not get.
        assert_eq!(
            role_onboarding_task(None, Some("Worker"), Some(profile)),
            role_onboarding_task(None, Some("Worker"), None),
            "a hostile profile must launch exactly like a plain Worker"
        );
    }
}

/// Separator, ordering and standalone behaviour are the parts a refactor is
/// most likely to drift on, so they are pinned literally.
#[test]
fn assignment_separators_and_ordering_are_pinned() {
    let orchestrator = role_onboarding_task(Some("do it"), Some("Manager"), Some("orchestrator"))
        .expect("orchestrator prompt");
    assert!(orchestrator.contains(&format!(
        "do it\n\n{}",
        crate::prompts::orchestrator_suffix()
    )));

    let planner = role_onboarding_task(Some("plan it"), Some("Manager"), Some("planner"))
        .expect("planner prompt");
    assert!(planner.contains(&format!("plan it{}", crate::prompts::planner_suffix())));

    // The Worker system prompt trims its assignment section; the manager paths
    // do not. Pin the trimmed form so the separator stays asserted either way.
    let worker =
        role_onboarding_task(Some("fix it"), Some("Worker"), None).expect("worker prompt");
    assert!(worker.contains(format!("fix it{}", crate::prompts::worker_suffix()).trim()));

    let watcher = role_onboarding_task(Some("watch it"), Some("Manager"), Some("watcher"))
        .expect("watcher prompt");
    assert!(!watcher.contains(crate::prompts::WORKER_SUFFIX));
    assert!(!watcher.contains(&crate::prompts::planner_suffix()));
}
