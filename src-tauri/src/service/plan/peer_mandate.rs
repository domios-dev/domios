//! Orchestrator<->Planner peer-mandate wiring (task_aa01c0db) on top of the
//! foundation peer-pairing primitives (`crate::plan::peer`, task_48f06a32):
//! the actual `--parent`/`--plan`-filled spawn command, the Planner's peer
//! intro line, and the nudge prefix shown while a plan is unpaired.

use crate::plan::peer::PeerResolution;

/// The canonical `agent spawn --role Planner --plan <id>` command, filled in
/// with a real orchestrator pane id when known (falls back to a placeholder
/// token otherwise, matching the `PLAN_ID`-style placeholders used elsewhere
/// in onboarding text).
pub fn planner_spawn_cmd_hint(orchestrator_pane_id: Option<&str>, plan_id: &str) -> String {
    let parent = orchestrator_pane_id.unwrap_or("<orch-pane>");
    format!("codeterm agent spawn claude --model fable --parent {parent} --role Planner --plan {plan_id}")
}

/// One-line onboarding text delivered to a freshly-spawned Planner naming its
/// Orchestrator peer, so the Planner knows who to hand structure back to.
pub fn planner_peer_intro_line(plan_id: &str, orchestrator_pane_id: Option<&str>) -> String {
    let orch = orchestrator_pane_id.unwrap_or("unknown");
    format!(
        "You are the Planner for plan {plan_id}; your Orchestrator peer is pane {orch}. {}",
        crate::prompts::PEER_PLANNER_CHARTER_LINE
    )
}

/// Compact prefix to prepend to an Orchestrator's plan reminder while the
/// plan has no live planner peer. `None` once paired or explicitly waived —
/// the nudge must stop immediately in both cases. A dead planner pane
/// (`OrchestratorFallback`) still nudges, since pairing has effectively
/// lapsed.
pub fn planner_pairing_nudge_prefix(resolution: &PeerResolution) -> Option<&'static str> {
    match resolution {
        PeerResolution::Unbound | PeerResolution::OrchestratorFallback { .. } => {
            Some(crate::prompts::PEER_WAIVER_HINT)
        }
        PeerResolution::Planner { .. } | PeerResolution::Waived { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nudge_prefix_fires_only_when_unpaired() {
        assert!(planner_pairing_nudge_prefix(&PeerResolution::Unbound).is_some());
        assert!(planner_pairing_nudge_prefix(&PeerResolution::OrchestratorFallback {
            bound_pane_id: "orch-1".into(),
            reason: "planner pane gone".into(),
        })
        .is_some());
        assert!(planner_pairing_nudge_prefix(&PeerResolution::Planner {
            pane_id: "p-1".into()
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
    }

    #[test]
    fn spawn_cmd_hint_falls_back_to_placeholder_without_a_real_pane() {
        let hint = planner_spawn_cmd_hint(None, "PLAN_ID");
        assert!(hint.contains("--parent <orch-pane>"));
        assert!(hint.contains("--plan PLAN_ID"));
    }

    #[test]
    fn peer_intro_line_names_the_orchestrator_peer() {
        let line = planner_peer_intro_line("pl_123", Some("orch-1"));
        assert!(line.contains("pl_123"));
        assert!(line.contains("orch-1"));
        assert!(line.contains(crate::prompts::PEER_PLANNER_CHARTER_LINE));
    }
}
