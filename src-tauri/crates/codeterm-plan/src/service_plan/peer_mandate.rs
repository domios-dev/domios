//! Orchestrator + Planner attachment wiring (task_aa01c0db) on top of the
//! foundation peer-pairing primitives (`crate::plan::peer`, task_48f06a32):
//! the actual `--parent`/`--plan`-filled spawn command, the Planner's
//! intro line, and the nudge prefix shown while a plan is unpaired.

use crate::plan::peer::PeerResolution;
use crate::plan::PlanningState;

/// Human label naming the attached Planner, derived from the resolved binding.
/// Used wherever planning work is delegated by name.
pub fn planner_peer_label(resolution: &PeerResolution) -> String {
    match resolution {
        PeerResolution::Planner { tab_id } => format!("the Planner specialist (tab {tab_id})"),
        PeerResolution::OrchestratorFallback { .. } => {
            "the Planner specialist (currently down — respawn it)".into()
        }
        PeerResolution::Waived { .. } => "yourself (planner waived — solo)".into(),
        PeerResolution::Unbound => "the Planner specialist (bind or spawn one)".into(),
    }
}

/// Structure-deferral nudge shown to an Orchestrator while the plan is not yet
/// approved. `None` once execution is unlocked (approved) or when the planner is
/// waived (solo — the orchestrator legitimately owns structure).
pub fn planning_structure_nudge(
    planning_state: PlanningState,
    resolution: &PeerResolution,
) -> Option<String> {
    if planning_state.allows_execution_tasks() {
        return None;
    }
    if matches!(resolution, PeerResolution::Waived { .. }) {
        return None;
    }
    Some(super::prompts_plan::orch_defer_structure_line(
        &planner_peer_label(resolution),
    ))
}

/// The canonical `agent spawn --role Planner --plan <id>` command, filled in
/// with a real orchestrator tab id when known (falls back to a placeholder
/// token otherwise, matching the `PLAN_ID`-style placeholders used elsewhere
/// in onboarding text).
pub fn planner_spawn_cmd_hint(orchestrator_tab_id: Option<&str>, plan_id: &str) -> String {
    let parent = orchestrator_tab_id.unwrap_or("<orch-tab>");
    format!(
        "Choose an available provider and model appropriate for this planning task, then run: codeterm agent spawn <provider> [--model <model>] --parent {parent} --role Planner --plan {plan_id}"
    )
}

/// One-line onboarding text delivered to a freshly-spawned Planner naming the
/// Orchestrator, so the Planner knows who owns final structure decisions.
pub fn planner_peer_intro_line(plan_id: &str, orchestrator_tab_id: Option<&str>) -> String {
    let orch = orchestrator_tab_id.unwrap_or("unknown");
    format!(
        "You are the Planner specialist for the active Team Plan {plan_id}; the Orchestrator owner is tab {orch}. {} The Orchestrator owns and creates the Team Plan and may create its structure directly. Team membership gives you the same active Plan automatically; no separate Planner binding or General Agent bootstrap is required. First read `codeterm docs PLANNER`. When planning is delegated, refine the active Team Plan and run its quality doctor. If no planning request has been delegated yet, reply that you are ready and WAIT.",
        super::prompts_plan::PEER_PLANNER_CHARTER_LINE
    )
}

/// Compact prefix to prepend to an Orchestrator's plan reminder while the
/// plan has no live Planner. `None` once attached or explicitly waived —
/// the nudge must stop immediately in both cases. A dead planner tab
/// (`OrchestratorFallback`) still nudges, since attachment has effectively
/// lapsed.
pub fn planner_pairing_nudge_prefix(resolution: &PeerResolution) -> Option<&'static str> {
    match resolution {
        PeerResolution::Unbound | PeerResolution::OrchestratorFallback { .. } => {
            Some(super::prompts_plan::PEER_WAIVER_HINT)
        }
        PeerResolution::Planner { .. } | PeerResolution::Waived { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_plan::prompts_plan;

    #[test]
    fn nudge_prefix_fires_only_when_unpaired() {
        assert!(planner_pairing_nudge_prefix(&PeerResolution::Unbound).is_some());
        assert!(
            planner_pairing_nudge_prefix(&PeerResolution::OrchestratorFallback {
                bound_tab_id: "orch-1".into(),
                reason: "planner tab gone".into(),
            })
            .is_some()
        );
        assert!(planner_pairing_nudge_prefix(&PeerResolution::Planner {
            tab_id: "p-1".into()
        })
        .is_none());
        assert!(planner_pairing_nudge_prefix(&PeerResolution::Waived {
            note: "solo".into()
        })
        .is_none());
    }

    #[test]
    fn spawn_cmd_hint_uses_real_ids_when_available() {
        let hint = planner_spawn_cmd_hint(Some("orch-1"), "pl_123");
        assert!(hint.contains("--parent orch-1"));
        assert!(hint.contains("--plan pl_123"));
        assert!(hint.contains("--role Planner"));
        assert!(hint.contains("Choose an available provider and model"));
        assert!(hint.contains("agent spawn <provider> [--model <model>]"));
        assert!(!hint.contains("agent spawn claude"));
        assert!(!hint.contains("fable"));
    }

    #[test]
    fn spawn_cmd_hint_falls_back_to_placeholder_without_a_real_tab() {
        let hint = planner_spawn_cmd_hint(None, "PLAN_ID");
        assert!(hint.contains("--parent <orch-tab>"));
        assert!(hint.contains("--plan PLAN_ID"));
    }

    #[test]
    fn peer_intro_line_names_the_orchestrator_owner() {
        let line = planner_peer_intro_line("pl_123", Some("orch-1"));
        assert!(line.contains("pl_123"));
        assert!(line.contains("orch-1"));
        assert!(line.contains(prompts_plan::PEER_PLANNER_CHARTER_LINE));
        assert!(line.contains("Orchestrator owner"));
    }

    #[test]
    fn planner_label_names_the_bound_tab() {
        let label = planner_peer_label(&PeerResolution::Planner {
            tab_id: "tab_p".into(),
        });
        assert!(label.contains("tab_p"));
    }

    #[test]
    fn structure_nudge_fires_only_while_unapproved_and_not_waived() {
        let live = PeerResolution::Planner {
            tab_id: "tab_p".into(),
        };
        // Unapproved + bound planner → nudge names the planner.
        let n = planning_structure_nudge(PlanningState::Drafting, &live).unwrap();
        assert!(n.contains("tab_p"));
        assert!(n.contains("delegate"));
        // Approved → silent (execution unlocked, structure owned).
        assert!(planning_structure_nudge(PlanningState::Approved, &live).is_none());
        // Waived → silent even while drafting (orchestrator is solo).
        assert!(planning_structure_nudge(
            PlanningState::Drafting,
            &PeerResolution::Waived {
                note: "solo".into()
            }
        )
        .is_none());
    }
}
