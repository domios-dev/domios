use crate::plan::reconcile::PaneProbe;
use crate::plan::{OrchestratorRef, PlannerRef};

/// Resolution result for a plan's `planner` field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PeerResolution {
    Planner {
        pane_id: String,
    },
    OrchestratorFallback {
        bound_pane_id: String,
        reason: String,
    },
    Waived {
        note: String,
    },
    Unbound,
}

impl Default for PeerResolution {
    fn default() -> Self {
        PeerResolution::Unbound
    }
}

/// Resolve `planner` to a live pane with orchestrator fallback.
/// An explicit `Waived` ref never falls through; it always reports `Waived`.
pub fn resolve_planner_ref<F>(
    planner: &Option<PlannerRef>,
    _orchestrator: &OrchestratorRef,
    pane_probe: F,
) -> PeerResolution
where
    F: Fn(&str) -> PaneProbe,
{
    match planner {
        None => PeerResolution::Unbound,
        Some(PlannerRef::Waived { note }) => PeerResolution::Waived { note: note.clone() },
        Some(PlannerRef::Pane { pane_id, .. }) => {
            let probe = pane_probe(pane_id);
            if probe.exists {
                PeerResolution::Planner {
                    pane_id: pane_id.clone(),
                }
            } else {
                PeerResolution::OrchestratorFallback {
                    bound_pane_id: pane_id.clone(),
                    reason: format!("planner pane {pane_id} absent"),
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

    fn idle_probe(_: &str) -> PaneProbe {
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

    fn pane(pane_id: &str) -> Option<PlannerRef> {
        Some(PlannerRef::Pane {
            pane_id: pane_id.into(),
            session_id: None,
            title: None,
        })
    }

    fn waived(note: &str) -> Option<PlannerRef> {
        Some(PlannerRef::Waived { note: note.into() })
    }

    fn resolution_json(resolution: &PeerResolution) -> String {
        serde_json::to_string(resolution).unwrap()
    }

    #[test]
    fn unbound_when_no_ref() {
        let r = resolve_planner_ref(&None, &Default::default(), gone_probe);
        assert_eq!(r, PeerResolution::Unbound);
    }

    #[test]
    fn planner_when_pane_live() {
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), live_probe);
        assert!(matches!(r, PeerResolution::Planner { pane_id } if pane_id == "pane_x"));
    }

    #[test]
    fn planner_when_pane_idle_but_present() {
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), idle_probe);
        assert!(matches!(r, PeerResolution::Planner { pane_id } if pane_id == "pane_x"));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"planner","pane_id":"pane_x"}"#
        );
    }

    #[test]
    fn fallback_when_planner_gone() {
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), gone_probe);
        assert!(matches!(
            r,
            PeerResolution::OrchestratorFallback { bound_pane_id, reason }
            if bound_pane_id == "pane_x" && reason == "planner pane pane_x absent"
        ));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"orchestrator_fallback","bound_pane_id":"pane_x","reason":"planner pane pane_x absent"}"#
        );
    }

    #[test]
    fn waived_never_falls_through() {
        let r = resolve_planner_ref(&waived("solo"), &Default::default(), live_probe);
        assert!(matches!(r, PeerResolution::Waived { note } if note == "solo"));
        let r2 = resolve_planner_ref(&waived("solo"), &Default::default(), gone_probe);
        assert!(matches!(r2, PeerResolution::Waived { .. }));
    }
}
