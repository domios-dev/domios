//! The plan and task flag fragment reaches exactly the surfaces that write plan
//! state. The fragment is an opaque token; its wording belongs to the .md file.

use crate::prompts::{CODETERM_CLI, GENERAL_AGENT_TEMPLATE, PLAN_TASK_FLAGS};
use crate::service::role_onboarding_task;
use crate::service::role_preset::builtin_guidance;

const ASSIGNMENT_TOKEN: &str = "OPAQUE-ASSIGNMENT-5b9e3";

fn fragment() -> &'static str {
    let token = PLAN_TASK_FLAGS.trim();
    assert!(!token.is_empty(), "an empty fragment proves nothing");
    token
}

#[test]
fn plan_task_flags_reach_orchestrator_and_planner_once() {
    let token = fragment();
    for (name, composed) in [
        (
            "orchestrator contract",
            builtin_guidance("orchestrator").expect("orchestrator"),
        ),
        (
            "planner contract",
            builtin_guidance("planner").expect("planner"),
        ),
        (
            "watcher contract",
            builtin_guidance("watcher").expect("watcher"),
        ),
        (
            "orchestrator launch",
            role_onboarding_task(None, Some("Orchestrator"), None, None)
                .expect("orchestrator launch"),
        ),
        (
            "planner launch",
            role_onboarding_task(
                Some(ASSIGNMENT_TOKEN),
                Some("Manager"),
                Some("planner"),
                None,
            )
            .expect("planner launch"),
        ),
    ] {
        assert_eq!(
            composed.matches(token).count(),
            1,
            "{name} must carry the plan flag fragment exactly once"
        );
        assert!(
            !composed.contains("{plan_task_flags}"),
            "{name} kept the slot"
        );
    }
}

#[test]
fn plan_task_flags_stay_off_general_and_worker_surfaces() {
    let token = fragment();
    let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
    let worker = role_onboarding_task(Some(ASSIGNMENT_TOKEN), Some("Worker"), None, None)
        .expect("worker launch");
    for (name, surface) in [
        ("codeterm cli", CODETERM_CLI.to_string()),
        ("general agent", general),
        (
            "worker contract",
            builtin_guidance("worker").expect("worker"),
        ),
        ("qa contract", builtin_guidance("qa").expect("qa")),
        (
            "cartographer contract",
            builtin_guidance("cartographer").expect("cartographer"),
        ),
        ("worker launch", worker),
    ] {
        assert!(
            !surface.contains(token),
            "{name} must not carry the plan flag fragment"
        );
    }
}
