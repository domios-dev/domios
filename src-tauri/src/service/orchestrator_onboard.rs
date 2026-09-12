//! Promote a tab to Orchestrator and inject the onboarding cheat-sheet.

use codeterm_types::team::general_agent_membership::MembershipAttempt;

use super::ServiceContext;
use crate::mem::{SearchHit, SearchQuery};
use crate::multiplexer::types::Tab;
use crate::service::general_agent::{refuse_general_agent_team_membership, shape_for_tab};
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

pub fn tab_is_agent_capable(tab: &Tab) -> bool {
    tab.agent_type.is_some() || tab.is_chat_backend()
}

pub fn validate_make_orchestrator(tab: &Tab) -> Result<(), String> {
    if tab.protected {
        return Err("Protected tab cannot become Orchestrator".into());
    }
    if !tab_is_agent_capable(tab) {
        return Err(
            "Only agent or chatBackend tabs can use Make Orchestrator (plain shells cannot)".into(),
        );
    }
    Ok(())
}

pub fn should_deliver_orchestrator_onboarding(tab: Option<&Tab>, was_orchestrator: bool) -> bool {
    if was_orchestrator {
        return false;
    }
    tab.map(|tab| tab.anchors_team() && tab_is_agent_capable(tab))
        .unwrap_or(false)
}

pub fn orchestrator_onboarding_body(tab_id: &str, team_id: &str, plan_id: Option<&str>) -> String {
    crate::prompts::orchestrator_first_message(tab_id, team_id, plan_id)
}

impl ServiceContext {
    pub fn deliver_orchestrator_onboarding(&self, tab_id: &str) {
        // Membership door 5: onboarding mints a Team onto the promoted Tab, and
        // the General Agent kind anchors, so it would qualify without this.
        {
            let mux = self.mux.read();
            let shape = shape_for_tab(&mux, tab_id);
            let guard =
                refuse_general_agent_team_membership(&mux, tab_id, MembershipAttempt::Join, &shape);
            if let Err(refusal) = guard {
                log::warn!("[orchestrator-onboard] {refusal}");
                return;
            }
        }
        // An anchoring Orchestrator owns a Team; mint it now if the promotion
        // left it unbound so the first message carries a real team id.
        let team_id = match self.mux.write().resolve_or_mint_team_for_parent(tab_id) {
            Ok(resolution) => resolution.team_id,
            Err(e) => {
                log::warn!("[orchestrator-onboard] team resolve for {tab_id} failed: {e}");
                return;
            }
        };
        // The team above is always freshly minted; scope the existence check
        // to the project instead — codeterm-releases#56.
        let project_path = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.cwd.clone())
                .or_else(|| {
                    mux.find_workspace_for_tab(tab_id)
                        .and_then(|workspace_id| mux.get_workspace(&workspace_id))
                        .and_then(|workspace| workspace.cwd.clone())
                })
        };
        let active_plan = project_path
            .as_deref()
            .and_then(|project_path| self.plan_store.active_plan_for_project(project_path));
        let plan_id = active_plan.as_ref().map(|(id, _)| id.as_str());
        let body = orchestrator_onboarding_body(tab_id, &team_id, plan_id);
        // Raw chat startup must see the full onboarding even before delivery lands.
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.pending_greeting = Some(body.clone());
        }
        self.emit_state_changed();
        // Deliver as a message FROM CodeTerm (same marker the Planner gets), not a
        // reminder — the onboarding should read as CodeTerm introducing the role.
        let envelope = MessageEnvelope::from_codeterm();
        if let Err(e) = self.gateway_deliver(
            tab_id,
            envelope,
            &body,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        ) {
            log::warn!("[orchestrator-onboard] deliver to {tab_id} failed: {e}");
        }
        self.deliver_orchestration_preferences(tab_id);
    }

    /// Off-hot-path pull of machine-local area=orchestration preferences,
    /// injected as a delta reminder. mem search is blocking (FTS + embeddings)
    /// so it runs on a blocking thread; empty result is a clean no-op.
    pub fn deliver_orchestration_preferences(&self, tab_id: &str) {
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        let job = move || {
            let hits = match svc.mem.search(&orchestration_preference_query()) {
                Ok(hits) => hits,
                Err(e) => {
                    log::warn!("[orchestration-prefs] mem search failed for {tab_id}: {e}");
                    return;
                }
            };
            let Some(body) = format_orchestration_preferences(&hits) else {
                return;
            };
            let envelope = MessageEnvelope::codeterm_reminder("orchestration-prefs");
            if let Err(e) = svc.gateway_deliver(
                &tab_id,
                envelope,
                &body,
                crate::service::message_gateway::DeliveryClass::SystemNotice,
            ) {
                log::warn!("[orchestration-prefs] deliver to {tab_id} failed: {e}");
            }
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(job);
        } else {
            std::thread::spawn(job);
        }
    }

    pub fn maybe_deliver_orchestrator_onboarding(&self, tab_id: &str) {
        let should_deliver = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .map(|tab| tab.anchors_team() && tab_is_agent_capable(tab))
                .unwrap_or(false)
        };
        if should_deliver {
            self.deliver_orchestrator_onboarding(tab_id);
        }
    }

    pub fn maybe_deliver_orchestrator_promotion_onboarding(
        &self,
        tab_id: &str,
        was_orchestrator: bool,
    ) {
        let should_deliver = {
            let mux = self.mux.read();
            should_deliver_orchestrator_onboarding(mux.get_tab(tab_id), was_orchestrator)
        };
        if should_deliver {
            self.deliver_orchestrator_onboarding(tab_id);
        }
    }

    pub fn make_orchestrator(&self, tab_id: &str) -> Result<(), String> {
        let was_orchestrator = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found"))?;
            validate_make_orchestrator(tab)?;
            tab.anchors_team()
        };

        self.transition_agent(crate::service::AgentTransitionRequest {
            tab_id: tab_id.to_string(),
            destination: crate::service::AgentPlacement::orchestrator(Some(true)),
            plan_binding: None,
            require_verified_session: true,
            planner_replacement: None,
            plan_rebind: None,
        })?;
        self.maybe_deliver_orchestrator_promotion_onboarding(tab_id, was_orchestrator);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::{AgentRole, TabBacking, TabType, TitleLevel};
    use codeterm_types::agent::role_kind::builtins::ORCHESTRATOR;

    fn mk_tab(id: &str) -> Tab {
        Tab {
            id: id.to_string(),
            sort_order: 0,
            pty_id: None,
            backing: TabBacking::default(),
            tab_type: TabType::Terminal,
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
            reasoning_effort_identity: Default::default(),
            context_epoch_archives: vec![],
            session_binding_nonce: None,
            reported_session_binding_nonce: None,
            prior_session_binding_nonces: Vec::new(),
            shell_integration_nonce: None,
            lifecycle_epoch: None,
            lifecycle_operation_id: None,
            task_binding: None,
            roleless_tracked: false,
            role: None,
            role_profile: None,
            team_id: None,
            watcher: None,
            parent_tab_id: None,
            pending_paired_planner_tab_id: None,
            can_spawn: false,
            depth: 0,
            report_target_tab_id: None,
            report_target_mesh_peer: None,
            spawned_by_tab_id: None,
            ownership: None,
            title_level: TitleLevel::Default,
            chat_view: false,
            pending_prompt: None,
            agent_state: None,
            auto_approve: false,
            protected: false,
            connected_services: vec![],
            hidden: false,
            audience: Default::default(),
            skip_snapshot: false,
            show_in_tabbar: true,
            worker_id: None,
            restored_from_snapshot: false,
            pending_greeting: None,
            boot_stage: None,
            boot_failure_reason: None,
            deadline_latched_launch: None,
        }
    }

    fn assert_no_placeholder_residue(msg: &str) {
        for token in [
            "PLAN_ID",
            "{planner_spawn_cmd}",
            "{orchestrator_suffix}",
            "{planning_compaction_contract}",
            "{infra_brief}",
        ] {
            assert!(!msg.contains(token), "first-message still carries {token}");
        }
        assert!(
            !msg.to_lowercase().contains("test planner"),
            "first-message names a phantom/example planner"
        );
    }

    #[test]
    fn plan_present_first_message_round_trips_real_ids() {
        let msg = orchestrator_onboarding_body("tab-abc", "team-xyz", Some("pl_real42"));
        // Round-trip: the ids the test supplied must echo verbatim.
        assert!(msg.contains("tab-abc"));
        assert!(msg.contains("team-xyz"));
        assert!(msg.contains("pl_real42"));
        // A ready-to-run planner spawn bound to THIS tab and the real plan.
        assert!(msg.contains("--parent-tab tab-abc --role Planner --plan pl_real42"));
        assert_no_placeholder_residue(&msg);
    }

    /// codeterm-releases#56: a freshly minted team must not blind the
    /// existence check to a plan the project already has.
    #[test]
    fn adoption_onboarding_does_not_emit_create_first_branch() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().display().to_string();
        let store = crate::service::plan::PlanStore::new_for_test(dir.path());
        // Real creation path canonicalizes project_path; raw Plan::new + save
        // skips that and never matches active_plan_for_project's query.
        let plan = store
            .create_plan_for_team(
                "Existing project plan".into(),
                project_path.clone(),
                crate::plan::OrchestratorRef::from_tab("prior-orch"),
                "team_existing_plan_owner".into(),
                vec![],
                1,
            )
            .unwrap();
        let plan_id = plan.id.clone();

        // A promotion always mints a FRESH, empty team — never one that could
        // already be bound to this (or any) plan.
        let fresh_team_id = "team_freshly_minted";
        assert_eq!(
            store.active_plan_for_team(fresh_team_id),
            None,
            "a freshly minted team must never already own a plan — this is why team-scoping is unsalvageable"
        );

        let resolved = store.active_plan_for_project(&project_path);
        assert_eq!(
            resolved.as_ref().map(|(id, _)| id.as_str()),
            Some(plan_id.as_str())
        );

        let msg = orchestrator_onboarding_body(
            "tab-abc",
            fresh_team_id,
            resolved.as_ref().map(|(id, _)| id.as_str()),
        );
        assert!(!msg.contains(crate::prompts::ORCH_ONBOARD_NO_PLAN_MARKER));
        assert!(!msg.contains(crate::prompts::ORCH_ONBOARD_CREATE_STEP));
        assert!(msg.contains(&plan_id));
    }

    #[test]
    fn plan_absent_first_message_emits_no_fake_plan_id() {
        let msg = orchestrator_onboarding_body("tab-abc", "team-xyz", None);
        assert!(msg.contains("tab-abc"));
        assert!(msg.contains("team-xyz"));
        // No fabricated plan id, and the create+bind path asserted by constant.
        assert!(!msg.contains("pl_"));
        assert!(msg.contains(crate::prompts::ORCH_ONBOARD_NO_PLAN_MARKER));
        assert!(msg.contains(crate::prompts::ORCH_ONBOARD_CREATE_STEP));
        // Binding is its own verb: `plan create` has no --team.
        assert!(msg.contains(&format!(
            "{} team-xyz",
            crate::prompts::ORCH_ONBOARD_ADOPT_STEP
        )));
        assert_no_placeholder_residue(&msg);
    }

    #[test]
    fn methodology_lives_in_system_prompt_not_first_message() {
        let doc = crate::prompts::ORCHESTRATION_DOC.trim();
        // System prompt path carries the manual; the first-message must not.
        assert!(crate::prompts::orchestrator_suffix().matches(doc).count() >= 1);
        for plan in [Some("pl_real42"), None] {
            let msg = orchestrator_onboarding_body("tab-abc", "team-xyz", plan);
            assert_eq!(
                msg.matches(doc).count(),
                0,
                "methodology duplicated into first-message"
            );
        }
    }

    #[test]
    fn plain_shell_is_ineligible() {
        let tab = mk_tab("shell-1");
        assert!(!tab_is_agent_capable(&tab));
        assert!(validate_make_orchestrator(&tab).is_err());
    }

    #[test]
    fn pty_agent_is_eligible() {
        let mut tab = mk_tab("agent-1");
        tab.agent_type = Some("claude".into());
        assert!(tab_is_agent_capable(&tab));
        assert!(validate_make_orchestrator(&tab).is_ok());
    }

    #[test]
    fn chat_backend_is_eligible() {
        let mut tab = mk_tab("chat-1");
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess-1".into(),
        };
        assert!(tab_is_agent_capable(&tab));
        assert!(validate_make_orchestrator(&tab).is_ok());
    }

    #[test]
    fn worker_child_can_transition_but_protected_stays_blocked() {
        let mut worker = mk_tab("w1");
        worker.agent_type = Some("claude".into());
        worker.role = Some(AgentRole::Worker);
        worker.parent_tab_id = Some("orch-1".into());
        assert!(validate_make_orchestrator(&worker).is_ok());

        let mut orch = mk_tab("o1");
        orch.agent_type = Some("claude".into());
        orch.role = Some(AgentRole::Manager);
        orch.role_profile = Some(ORCHESTRATOR.into());
        assert!(validate_make_orchestrator(&orch).is_ok());

        let mut protected = mk_tab("p1");
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
        let mut tab = mk_tab("agent-1");
        tab.agent_type = Some("claude".into());
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some(ORCHESTRATOR.into());
        assert!(should_deliver_orchestrator_onboarding(Some(&tab), false));
        assert!(!should_deliver_orchestrator_onboarding(Some(&tab), true));

        let mut shell = mk_tab("shell-1");
        shell.role = Some(AgentRole::Manager);
        shell.role_profile = Some(ORCHESTRATOR.into());
        assert!(!should_deliver_orchestrator_onboarding(Some(&shell), false));
        assert!(!should_deliver_orchestrator_onboarding(None, false));
    }
}
