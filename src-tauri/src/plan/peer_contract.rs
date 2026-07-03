use crate::plan::reconcile::PaneProbe;
use crate::plan::{OrchestratorRef, PlannerRef};

/// Planner owns GOAL, Spec, and Plan structure. The orchestrator defers structural
/// decisions (sections, tasks, sequencing) to the Planner peer.
pub const PLANNER_OWNS_STRUCTURE: &str = "\
Planner owns GOAL, Spec, and Plan structure. \
The orchestrator defers structural decisions (sections, tasks, sequencing) \
to the Planner peer.";

/// Instruction block for the orchestrator when a planner peer is bound.
pub const ORCHESTRATOR_DEFERS_TO_PLANNER: &str = "\
As orchestrator, defer to the Planner peer for structural decisions. \
Do not add sections, reorganize tasks, or change the plan skeleton \
without Planner acknowledgment.";

/// Skeleton block consumers embed in peer-pairing prompts.
pub const PAIRING_MANDATE_BLOCK: &str = "\
## Peer contract

This plan has a Planner peer bound via `planner_ref`. \
The orchestrator is the execution authority; the Planner is the structural authority. \
Structural changes require Planner acknowledgment.";

/// Resolution result for a plan's `planner_ref`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannerResolution {
    Live {
        pane_id: String,
    },
    FallbackOrchestrator {
        bound_pane_id: String,
        reason: String,
    },
    Waived {
        note: String,
    },
    Unset,
}

impl Default for PlannerResolution {
    fn default() -> Self {
        PlannerResolution::Unset
    }
}

/// Resolve `planner_ref` to a live pane with orchestrator fallback.
/// An explicit `Waived` ref never falls through; it always reports `Waived`.
pub fn resolve_planner<F>(
    planner_ref: &Option<PlannerRef>,
    _orchestrator: &OrchestratorRef,
    pane_probe: F,
) -> PlannerResolution
where
    F: Fn(&str) -> PaneProbe,
{
    match planner_ref {
        None => PlannerResolution::Unset,
        Some(PlannerRef::Waived { note }) => PlannerResolution::Waived { note: note.clone() },
        Some(PlannerRef::Bound { pane_id, .. }) => {
            let probe = pane_probe(pane_id);
            if probe.live_agent {
                PlannerResolution::Live {
                    pane_id: pane_id.clone(),
                }
            } else {
                let reason = if probe.exists {
                    format!("planner pane {pane_id} not live")
                } else {
                    format!("planner pane {pane_id} gone")
                };
                PlannerResolution::FallbackOrchestrator {
                    bound_pane_id: pane_id.clone(),
                    reason,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::reconcile::PaneProbe;

    fn live_probe(_: &str) -> PaneProbe {
        PaneProbe {
            exists: true,
            live_agent: true,
            role: None,
            parent_pane_id: None,
        }
    }

    fn dead_probe(_: &str) -> PaneProbe {
        PaneProbe {
            exists: true,
            live_agent: false,
            role: None,
            parent_pane_id: None,
        }
    }

    fn gone_probe(_: &str) -> PaneProbe {
        PaneProbe::default()
    }

    fn bound(pane_id: &str) -> Option<PlannerRef> {
        Some(PlannerRef::Bound {
            pane_id: pane_id.into(),
            session_id: None,
            title: None,
        })
    }

    fn waived(note: &str) -> Option<PlannerRef> {
        Some(PlannerRef::Waived { note: note.into() })
    }

    #[test]
    fn unset_when_no_ref() {
        let r = resolve_planner(&None, &Default::default(), gone_probe);
        assert_eq!(r, PlannerResolution::Unset);
    }

    #[test]
    fn live_when_planner_active() {
        let r = resolve_planner(&bound("pane_x"), &Default::default(), live_probe);
        assert!(matches!(r, PlannerResolution::Live { pane_id } if pane_id == "pane_x"));
    }

    #[test]
    fn fallback_when_planner_dead() {
        let r = resolve_planner(&bound("pane_x"), &Default::default(), dead_probe);
        assert!(matches!(r, PlannerResolution::FallbackOrchestrator { .. }));
    }

    #[test]
    fn fallback_when_planner_gone() {
        let r = resolve_planner(&bound("pane_x"), &Default::default(), gone_probe);
        assert!(matches!(r, PlannerResolution::FallbackOrchestrator { .. }));
    }

    #[test]
    fn waived_never_falls_through() {
        let r = resolve_planner(&waived("solo"), &Default::default(), live_probe);
        assert!(matches!(r, PlannerResolution::Waived { note } if note == "solo"));
        let r2 = resolve_planner(&waived("solo"), &Default::default(), gone_probe);
        assert!(matches!(r2, PlannerResolution::Waived { .. }));
    }
}
