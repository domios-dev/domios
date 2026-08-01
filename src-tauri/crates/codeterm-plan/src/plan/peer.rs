use crate::plan::reconcile::TabProbe;
use crate::plan::{OrchestratorRef, PlannerRef};

/// Resolution result for a plan's `planner` field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PeerResolution {
    Planner {
        tab_id: String,
    },
    OrchestratorFallback {
        bound_tab_id: String,
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

/// Resolve `planner` to a live tab with orchestrator fallback.
/// An explicit `Waived` ref never falls through; it always reports `Waived`.
pub fn resolve_planner_ref<F>(
    planner: &Option<PlannerRef>,
    _orchestrator: &OrchestratorRef,
    tab_probe: F,
) -> PeerResolution
where
    F: Fn(&str) -> TabProbe,
{
    match planner {
        None => PeerResolution::Unbound,
        Some(PlannerRef::Waived { note }) => PeerResolution::Waived { note: note.clone() },
        Some(PlannerRef::Tab {
            tab_id, session_id, ..
        }) => {
            let probe = tab_probe(tab_id);
            if probe.planner_routable_for(session_id.as_deref()) {
                PeerResolution::Planner {
                    tab_id: tab_id.clone(),
                }
            } else {
                let reason = if probe.exists {
                    format!(
                        "planner_dead: tab {tab_id} has no nonce-verified live agent chat session"
                    )
                } else {
                    format!("planner tab {tab_id} gone")
                };
                PeerResolution::OrchestratorFallback {
                    bound_tab_id: tab_id.clone(),
                    reason,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::reconcile::TabProbe;

    fn live_probe(_: &str) -> TabProbe {
        TabProbe {
            exists: true,
            live_agent: true,
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: true,
            agent_accepting_work: true,
            role: None,
            parent_tab_id: None,
        }
    }

    fn idle_probe(_: &str) -> TabProbe {
        TabProbe {
            exists: true,
            live_agent: true,
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: true,
            agent_accepting_work: true,
            role: None,
            parent_tab_id: None,
        }
    }

    fn gone_probe(_: &str) -> TabProbe {
        TabProbe::default()
    }

    fn tab(tab_id: &str) -> Option<PlannerRef> {
        Some(PlannerRef::Tab {
            tab_id: tab_id.into(),
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
    fn planner_when_tab_live() {
        let r = resolve_planner_ref(&tab("tab_x"), &Default::default(), live_probe);
        assert!(matches!(r, PeerResolution::Planner { ref tab_id } if tab_id == "tab_x"));
    }

    #[test]
    fn planner_when_tab_idle_but_present() {
        let r = resolve_planner_ref(&tab("tab_x"), &Default::default(), idle_probe);
        assert!(matches!(r, PeerResolution::Planner { ref tab_id } if tab_id == "tab_x"));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"planner","tab_id":"tab_x"}"#
        );
    }

    #[test]
    fn live_tab_or_pty_without_agent_chat_is_planner_dead() {
        let probe = TabProbe {
            exists: true,
            live_agent: true,
            agent_session_id: Some("session-1".into()),
            session_binding_valid: true,
            chat_delivery_target: false,
            agent_accepting_work: false,
            role: None,
            parent_tab_id: None,
        };
        let r = resolve_planner_ref(&tab("tab_x"), &Default::default(), |_| probe.clone());
        assert!(
            matches!(r, PeerResolution::OrchestratorFallback { reason, .. } if reason.contains("planner_dead"))
        );
    }

    #[test]
    fn planner_session_must_match_the_bound_session() {
        let bound = Some(PlannerRef::Tab {
            tab_id: "tab_x".into(),
            session_id: Some("old-session".into()),
            title: None,
        });
        let r = resolve_planner_ref(&bound, &Default::default(), live_probe);
        assert!(matches!(r, PeerResolution::OrchestratorFallback { .. }));
    }

    #[test]
    fn stale_ref_without_bound_session_is_not_routable() {
        let stale = Some(PlannerRef::Tab {
            tab_id: "tab_x".into(),
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
        let r = resolve_planner_ref(&tab("tab_x"), &Default::default(), gone_probe);
        assert!(matches!(
            r,
            PeerResolution::OrchestratorFallback { ref bound_tab_id, ref reason }
            if bound_tab_id == "tab_x" && reason == "planner tab tab_x gone"
        ));
        assert_eq!(
            resolution_json(&r),
            r#"{"kind":"orchestrator_fallback","bound_tab_id":"tab_x","reason":"planner tab tab_x gone"}"#
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
