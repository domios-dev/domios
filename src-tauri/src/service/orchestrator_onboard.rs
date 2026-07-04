//! Promote a pane to Orchestrator and inject the onboarding cheat-sheet.

use super::ServiceContext;
use crate::multiplexer::types::{AgentRole, Pane};
use crate::service::messenger::{MessageEnvelope, Messenger};

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
    // No plan is claimed yet at promotion time, so PLAN_ID stays a
    // placeholder token — consistent with the other PLAN_ID placeholders
    // below — until the Orchestrator claims/creates a real plan.
    let spawn_cmd =
        crate::service::plan::peer_mandate::planner_spawn_cmd_hint(Some(pane_id), "PLAN_ID");
    let mandate = crate::prompts::peer_pairing_mandate_full(&spawn_cmd);
    format!(
        "You are now the Orchestrator for this pane (role=Orchestrator, can_spawn=true).\n\
\n\
First load the orchestration rules and local preferences:\n\
  codeterm docs ORCHESTRATION\n\
  codeterm mem search \"CodeTerm planner orchestration preferences\"\n\
\n\
Planning rule: CodeTerm planner is the source of truth. Set GOAL first; put WHAT/WHY in Spec, HOW in Plan sections, and concrete work in Tasks. Do not create markdown implementation plans as the primary plan unless the user explicitly asks for an export.\n\
\n\
{mandate}\n\
\n\
Full orchestration toolset reference:\n\
  codeterm doc orchestration\n\
  (alias: codeterm docs ORCHESTRATION)\n\
\n\
After you own a plan — status, lanes, drift, next actions:\n\
  codeterm orchestrator status --plan PLAN_ID\n\
\n\
Keep heartbeat current (auto-reinjected on nudges):\n\
  codeterm orchestrator heartbeat set --plan PLAN_ID --focus \"...\"\n\
\n\
Add and assign work:\n\
  codeterm plan task add PLAN_ID --name \"...\" --why \"...\"\n\
  codeterm plan task assign PLAN_ID TASK_ID --pane WORKER_PANE\n\
\n\
Spawn workers (2-level hierarchy — Workers report back here):\n\
  codeterm agent spawn --parent {pane_id} --role Worker --title \"...\" --task \"...\""
    )
}

impl ServiceContext {
    pub fn deliver_orchestrator_onboarding(&self, pane_id: &str) {
        let body = orchestrator_onboarding_body(pane_id);
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.pending_greeting = Some(body.clone());
        }
        self.emit_state_changed();
        let envelope = MessageEnvelope::reminder("CodeTerm", "orchestrator-onboard");
        if let Err(e) = Messenger::deliver(self, pane_id, envelope, &body) {
            log::warn!("[orchestrator-onboard] deliver to {pane_id} failed: {e}");
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
            title: "Terminal".to_string(),
            description: None,
            cwd: None,
            url: None,
            agent_type: None,
            agent_type_locked: false,
            agent_set_at: None,
            exit_suspected_at: None,
            agent_booting: false,
            agent_session_id: None,
            agent_session_name: None,
            agent_session_model: None,
            session_binding_nonce: None,
            shell_integration_nonce: None,
            role: None,
            role_profile: None,
            parent_pane_id: None,
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
        }
    }

    #[test]
    fn onboarding_body_lists_canonical_commands() {
        let msg = orchestrator_onboarding_body("pane-abc");
        assert!(msg.contains("You are now the Orchestrator"));
        assert!(msg.contains("CodeTerm planner is the source of truth"));
        assert!(msg.contains("codeterm mem search \"CodeTerm planner orchestration preferences\""));
        assert!(msg.contains("codeterm doc orchestration"));
        assert!(msg.contains("codeterm docs ORCHESTRATION"));
        assert!(msg.contains("codeterm orchestrator status --plan PLAN_ID"));
        assert!(msg.contains("codeterm orchestrator heartbeat set"));
        assert!(msg.contains("codeterm plan task add PLAN_ID"));
        assert!(msg.contains("codeterm plan task assign PLAN_ID TASK_ID"));
        assert!(msg.contains("codeterm agent spawn --parent pane-abc --role Worker"));
    }

    #[test]
    fn onboarding_body_carries_peer_pairing_mandate_with_real_pane_id() {
        let msg = orchestrator_onboarding_body("pane-abc");
        assert!(msg.contains(crate::prompts::PEER_PLANNER_CHARTER_LINE));
        assert!(msg.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
        assert!(msg.contains("attached Planner specialist"));
        assert!(msg.contains("--parent pane-abc --role Planner --plan PLAN_ID"));
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
