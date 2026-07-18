//! Promote a pane to Orchestrator and inject the onboarding cheat-sheet.

use super::ServiceContext;
use crate::mem::{SearchHit, SearchQuery};
use crate::multiplexer::types::{AgentRole, Pane};
use crate::service::messenger::MessageEnvelope;

const PREF_QUERY: &str = "CodeTerm orchestration and planning preferences";
const PREF_LIMIT: usize = 5;
const PREF_ITEM_CAP: usize = 400;
const PREF_TOTAL_CAP: usize = 2000;

pub fn orchestration_preference_query() -> SearchQuery {
    SearchQuery {
        query: PREF_QUERY.to_string(),
        kind: Some("preference".to_string()),
        scope: None,
        area: Some("orchestration".to_string()),
        limit: Some(PREF_LIMIT),
    }
}

/// Bounded English delta block from area=orchestration preferences. None when empty.
pub fn format_orchestration_preferences(hits: &[SearchHit]) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Machine-local orchestration preferences (mem, area=orchestration) — apply as deltas over the ORCHESTRATION doc:",
    );
    for hit in hits {
        let body = hit.item.body.trim();
        let text: &str = if body.is_empty() {
            hit.item.summary.as_deref().unwrap_or("").trim()
        } else {
            body
        };
        if text.is_empty() {
            continue;
        }
        let mut clipped: String = text.chars().take(PREF_ITEM_CAP).collect();
        if clipped.len() < text.len() {
            clipped.push('…');
        }
        let title = hit.item.title.trim();
        let line = if title.is_empty() {
            format!("\n- {clipped}")
        } else {
            format!("\n- {title}: {clipped}")
        };
        if out.len() + line.len() > PREF_TOTAL_CAP {
            break;
        }
        out.push_str(&line);
    }
    if out.contains("\n- ") {
        Some(out)
    } else {
        None
    }
}

pub fn pane_is_agent_capable(pane: &Pane) -> bool {
    pane.agent_type.is_some() || pane.is_chat_backend()
}

pub fn validate_make_orchestrator(pane: &Pane) -> Result<(), String> {
    if pane.protected {
        return Err("Protected pane cannot become Orchestrator".into());
    }
    if pane.role == Some(AgentRole::Orchestrator) {
        return Err("Pane is already an Orchestrator".into());
    }
    if pane.parent_pane_id.is_some() {
        return Err("Cannot become Orchestrator — already in a hierarchy".into());
    }
    if !pane_is_agent_capable(pane) {
        return Err(
            "Only agent or chatBackend panes can use Make Orchestrator (plain shells cannot)"
                .into(),
        );
    }
    Ok(())
}

pub fn should_deliver_orchestrator_onboarding(pane: Option<&Pane>, was_orchestrator: bool) -> bool {
    if was_orchestrator {
        return false;
    }
    pane.map(|pane| pane.role == Some(AgentRole::Orchestrator) && pane_is_agent_capable(pane))
        .unwrap_or(false)
}

pub fn orchestrator_onboarding_body(pane_id: &str) -> String {
    let spawn_cmd =
        crate::service::plan::peer_mandate::planner_spawn_cmd_hint(Some(pane_id), "PLAN_ID");
    crate::prompts::orchestrator_onboarding(&spawn_cmd)
}

impl ServiceContext {
    pub fn deliver_orchestrator_onboarding(&self, pane_id: &str) {
        let body = orchestrator_onboarding_body(pane_id);
        // Raw chat startup must see the full onboarding even before delivery lands.
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.pending_greeting = Some(body.clone());
        }
        self.emit_state_changed();
        // Deliver as a message FROM CodeTerm (same marker the Planner gets), not a
        // reminder — the onboarding should read as CodeTerm introducing the role.
        let envelope = MessageEnvelope::from_codeterm();
        if let Err(e) = self.gateway_deliver(
            pane_id,
            envelope,
            &body,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        ) {
            log::warn!("[orchestrator-onboard] deliver to {pane_id} failed: {e}");
        }
        self.deliver_orchestration_preferences(pane_id);
    }

    /// Off-hot-path pull of machine-local area=orchestration preferences,
    /// injected as a delta reminder. mem search is blocking (FTS + embeddings)
    /// so it runs on a blocking thread; empty result is a clean no-op.
    pub fn deliver_orchestration_preferences(&self, pane_id: &str) {
        let svc = self.clone();
        let pane_id = pane_id.to_string();
        let job = move || {
            let hits = match svc.mem.search(&orchestration_preference_query()) {
                Ok(hits) => hits,
                Err(e) => {
                    log::warn!("[orchestration-prefs] mem search failed for {pane_id}: {e}");
                    return;
                }
            };
            let Some(body) = format_orchestration_preferences(&hits) else {
                return;
            };
            let envelope = MessageEnvelope::codeterm_reminder("orchestration-prefs");
            if let Err(e) = svc.gateway_deliver(
                &pane_id,
                envelope,
                &body,
                crate::service::message_gateway::DeliveryClass::SystemNotice,
            ) {
                log::warn!("[orchestration-prefs] deliver to {pane_id} failed: {e}");
            }
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(job);
        } else {
            std::thread::spawn(job);
        }
    }

    pub fn maybe_deliver_orchestrator_onboarding(&self, pane_id: &str) {
        let should_deliver = {
            let mux = self.mux.read();
            mux.get_pane(pane_id)
                .map(|pane| {
                    pane.role == Some(AgentRole::Orchestrator) && pane_is_agent_capable(pane)
                })
                .unwrap_or(false)
        };
        if should_deliver {
            self.deliver_orchestrator_onboarding(pane_id);
        }
    }

    pub fn maybe_deliver_orchestrator_promotion_onboarding(
        &self,
        pane_id: &str,
        was_orchestrator: bool,
    ) {
        let should_deliver = {
            let mux = self.mux.read();
            should_deliver_orchestrator_onboarding(mux.get_pane(pane_id), was_orchestrator)
        };
        if should_deliver {
            self.deliver_orchestrator_onboarding(pane_id);
        }
    }

    pub fn make_orchestrator(&self, pane_id: &str) -> Result<(), String> {
        {
            let mux = self.mux.read();
            let pane = mux
                .get_pane(pane_id)
                .ok_or_else(|| format!("Pane {pane_id} not found"))?;
            validate_make_orchestrator(pane)?;
        }

        self.set_pane_role(pane_id, "Orchestrator", None, Some(true))?;
        self.deliver_orchestrator_onboarding(pane_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::{PaneBacking, PaneType, TitleLevel};

    fn mk_pane(id: &str) -> Pane {
        Pane {
            id: id.to_string(),
            pty_id: None,
            backing: PaneBacking::default(),
            pane_type: PaneType::Terminal,
            pending_delivery: None,
            title: "Terminal".to_string(),
            description: None,
            cwd: None,
            provisioned_cwd: None,
            url: None,
            agent_type: None,
            agent_type_locked: false,
            agent_set_at: None,
            exit_suspected_at: None,
            agent_booting: false,
            agent_session_id: None,
            agent_session_name: None,
            agent_session_model: None,
            agent_planned_model: None,
            agent_requested_model: None,
            agent_provider_resolved_model: None,
            model_fallback: None,
            model_identity_mismatch: None,
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            shell_integration_nonce: None,
            role: None,
            role_profile: None,
            team_id: None,
            watcher: None,
            parent_pane_id: None,
            pending_paired_planner_pane_id: None,
            can_spawn: false,
            depth: 0,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            spawned_by_pane_id: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            agent_state: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            hidden: false,
            skip_snapshot: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
        }
    }

    #[test]
    fn onboarding_body_is_bounded_and_fully_rendered() {
        let msg = orchestrator_onboarding_body("pane-abc");
        assert!(msg.split_whitespace().count() < 800);
        assert!(msg.contains("--parent pane-abc --role Planner --plan PLAN_ID"));
        assert!(!msg.contains("{planner_spawn_cmd}"));
        assert!(!msg.contains("{orchestrator_suffix}"));
    }

    #[test]
    fn plain_shell_is_ineligible() {
        let pane = mk_pane("shell-1");
        assert!(!pane_is_agent_capable(&pane));
        assert!(validate_make_orchestrator(&pane).is_err());
    }

    #[test]
    fn pty_agent_is_eligible() {
        let mut pane = mk_pane("agent-1");
        pane.agent_type = Some("claude".into());
        assert!(pane_is_agent_capable(&pane));
        assert!(validate_make_orchestrator(&pane).is_ok());
    }

    #[test]
    fn chat_backend_is_eligible() {
        let mut pane = mk_pane("chat-1");
        pane.backing = PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-1".into(),
        };
        assert!(pane_is_agent_capable(&pane));
        assert!(validate_make_orchestrator(&pane).is_ok());
    }

    #[test]
    fn worker_child_and_protected_still_blocked() {
        let mut worker = mk_pane("w1");
        worker.agent_type = Some("claude".into());
        worker.role = Some(AgentRole::Worker);
        worker.parent_pane_id = Some("orch-1".into());
        assert!(validate_make_orchestrator(&worker).is_err());

        let mut orch = mk_pane("o1");
        orch.agent_type = Some("claude".into());
        orch.role = Some(AgentRole::Orchestrator);
        assert!(validate_make_orchestrator(&orch).is_err());

        let mut protected = mk_pane("p1");
        protected.agent_type = Some("claude".into());
        protected.protected = true;
        assert!(validate_make_orchestrator(&protected).is_err());
    }

    fn mk_hit(title: &str, body: &str) -> SearchHit {
        use crate::mem::store::MemItem;
        SearchHit {
            item: MemItem {
                id: "id".into(),
                uid: "uid".into(),
                kind: "preference".into(),
                scope: "global".into(),
                title: title.into(),
                summary: None,
                area: Some("orchestration".into()),
                tags: vec![],
                body: body.into(),
                source: None,
                importance: 0.0,
                created_at: String::new(),
                updated_at: String::new(),
                accessed_at: None,
                access_count: 0,
                recurrence_count: 0,
            },
            score: 1.0,
        }
    }

    #[test]
    fn preference_query_is_scoped_to_orchestration_preferences() {
        let q = orchestration_preference_query();
        assert_eq!(q.kind.as_deref(), Some("preference"));
        assert_eq!(q.area.as_deref(), Some("orchestration"));
        assert!(q.limit.is_some());
    }

    #[test]
    fn empty_preferences_produce_no_injection() {
        assert!(format_orchestration_preferences(&[]).is_none());
    }

    #[test]
    fn preferences_format_bounded_english_block() {
        let hits = vec![
            mk_hit("Build cap", "Only two concurrent QA gates on this machine"),
            mk_hit("", "x".repeat(5000).as_str()),
        ];
        let block = format_orchestration_preferences(&hits).expect("non-empty");
        assert!(block.contains("area=orchestration"));
        assert!(block.contains("Build cap"));
        assert!(block.len() <= PREF_TOTAL_CAP);
    }

    #[test]
    fn blank_bodies_are_skipped() {
        let hits = vec![mk_hit("empty", "   ")];
        assert!(format_orchestration_preferences(&hits).is_none());
    }

    #[test]
    fn onboarding_delivery_predicate_requires_first_agent_orchestrator() {
        let mut pane = mk_pane("agent-1");
        pane.agent_type = Some("claude".into());
        pane.role = Some(AgentRole::Orchestrator);
        assert!(should_deliver_orchestrator_onboarding(Some(&pane), false));
        assert!(!should_deliver_orchestrator_onboarding(Some(&pane), true));

        let mut shell = mk_pane("shell-1");
        shell.role = Some(AgentRole::Orchestrator);
        assert!(!should_deliver_orchestrator_onboarding(Some(&shell), false));
        assert!(!should_deliver_orchestrator_onboarding(None, false));
    }
}
