//! What a manager surface may say when it has nothing to do yet. Split out of
//! `tests.rs` to keep that file inside the repository size gate.

use crate::prompts::PLANNER_SUFFIX;

const TEAM_BOOTSTRAP_STARTER: &str = include_str!("../../../prompts/team_bootstrap_starter.md");

/// A freshly paired manager wakes up with nothing to do. Both idle surfaces
/// must keep that acknowledgement inside its own tab: a readiness ping to the
/// counterpart carries no decision, evidence or state change, and it costs the
/// receiver a full turn before the user has stated a goal.
#[test]
fn idle_wake_up_surfaces_keep_the_acknowledgement_in_the_tab() {
    for (name, surface, ban) in [
        (
            "team_bootstrap_starter",
            TEAM_BOOTSTRAP_STARTER,
            "do not message your counterpart",
        ),
        (
            "planner_suffix",
            PLANNER_SUFFIX,
            "do not message the Orchestrator",
        ),
    ] {
        assert!(
            surface.contains(ban),
            "{name} must forbid the unsolicited readiness ping"
        );
        assert!(
            surface.contains("in this tab") || surface.contains("in your own tab"),
            "{name} must place the acknowledgement in the agent's own tab"
        );
    }
}

/// Silencing only the wake-up leaves the same defect available every later
/// turn. The Planner surface must state the standing cost of a message, not
/// just the one it is forbidden to send at launch.
#[test]
fn planner_surface_prices_every_message_to_the_orchestrator() {
    assert!(
        PLANNER_SUFFIX
            .contains("a bare acknowledgement, a restatement, or a readiness note is not sent at all"),
        "the Planner surface must refuse contentless messages for the whole session, not only at wake-up"
    );
}
