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
        Some(PlannerRef::Pane {
            pane_id,
            session_id,
            ..
        }) => {
            let probe = pane_probe(pane_id);
            if probe.planner_routable_for(session_id.as_deref()) {
                PeerResolution::Planner {
                    pane_id: pane_id.clone(),
                }
            } else {
                let reason = if probe.exists {
                    format!("planner_dead: pane {pane_id} has no nonce-verified live agent chat session")
                } else {
                    format!("planner pane {pane_id} gone")
                };
                PeerResolution::OrchestratorFallback {
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
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: true,
            agent_accepting_work: true,
            role: None,
            parent_pane_id: None,
        }
    }

    fn idle_probe(_: &str) -> PaneProbe {
        PaneProbe {
            exists: true,
            live_agent: true,
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: true,
            agent_accepting_work: true,
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
            session_id: Some("session-1".into()),
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
        assert!(matches!(r, PeerResolution::Planner { ref pane_id } if pane_id == "pane_x"));
    }

    #[test]
    fn planner_when_pane_idle_but_present() {
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), idle_probe);
        assert!(matches!(r, PeerResolution::Planner { ref pane_id } if pane_id == "pane_x"));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"planner","pane_id":"pane_x"}"#
        );
    }

    #[test]
    fn live_pane_or_pty_without_agent_chat_is_planner_dead() {
        let probe = PaneProbe {
            exists: true,
            live_agent: true,
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: false,
            agent_accepting_work: false,
            role: None,
            parent_pane_id: None,
        };
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), |_| probe.clone());
        assert!(
            matches!(r, PeerResolution::OrchestratorFallback { reason, .. } if reason.contains("planner_dead"))
        );
    }

    #[test]
    fn planner_session_must_match_the_bound_session() {
        let bound = Some(PlannerRef::Pane {
            pane_id: "pane_x".into(),
            session_id: Some("old-session".into()),
            title: None,
        });
        let r = resolve_planner_ref(&bound, &Default::default(), live_probe);
        assert!(matches!(r, PeerResolution::OrchestratorFallback { .. }));
    }

    #[test]
    fn stale_ref_without_bound_session_is_not_routable() {
        let stale = Some(PlannerRef::Pane {
            pane_id: "pane_x".into(),
            session_id: None,
            title: None,
        });
        let resolution = resolve_planner_ref(&stale, &Default::default(), live_probe);
        assert!(matches!(
            resolution,
            PeerResolution::OrchestratorFallback { .. }
        ));
    }

    #[test]
    fn fallback_when_planner_gone() {
        let r = resolve_planner_ref(&pane("pane_x"), &Default::default(), gone_probe);
        assert!(matches!(
            r,
            PeerResolution::OrchestratorFallback { ref bound_pane_id, ref reason }
            if bound_pane_id == "pane_x" && reason == "planner pane pane_x gone"
        ));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"orchestrator_fallback","bound_pane_id":"pane_x","reason":"planner pane pane_x gone"}"#
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
