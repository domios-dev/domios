//! Paired creation (task_7e5f9354): spawn an Orchestrator + Planner together,
//! linked from birth by tab hierarchy; the plan is created lazily on the
//! Orchestrator's first goal/plan create. Builds entirely on the
//! planner-attachment primitives already wired by the peer-foundation
//! (`crate::plan::peer`) and Lane 1 (`crate::service::plan::peer_mandate`,
//! Manager + planner binding in `Self::spawn_agent`) - this module
//! only sequences the two spawns and records the pending pair.

use codeterm_types::agent::role_kind::builtins::{ORCHESTRATOR, PLANNER};
use serde::{Deserialize, Serialize};

use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::messenger::MessageEnvelope;
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::ServiceContext;

const PAIR_AGENT_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
const PAIR_API_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(75);

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OrchestrationPairParams {
    pub orchestrator_agent: String,
    pub orchestrator_model: Option<String>,
    pub planner_agent: String,
    pub planner_model: Option<String>,
    pub workspace_id: Option<String>,
    pub spawned_by_tab_id: Option<String>,
    /// Blank picker tab to claim for the Orchestrator in place, instead of
    /// spawning it as a brand-new tab (new-session picker confirm path).
    pub existing_tab_id: Option<String>,
}

/// Success shape only — a failed Planner spawn returns `Err(String)` with the
/// exact recovery command embedded (see `create_orchestration_pair`), never a
/// half-populated `Ok`. There is no silent half-pair to represent here.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationPairResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    pub orchestrator_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_model: Option<String>,
    pub planner_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Advisory tab-budget nudge (task_4d649dfd) for the workspace AFTER both
    /// tabs land — a pair adds two, so its count is the authoritative one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<crate::service::tab_budget::TabBudgetNotice>,
}

/// The exact command to finish a degraded pair by hand, using the real
/// requested provider and model rather than a hardcoded default.
fn degraded_fix_command(
    orch_tab_id: &str,
    planner_agent: &str,
    planner_model: Option<&str>,
) -> String {
    let model_flag = planner_model
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!(" --model {m}"))
        .unwrap_or_default();
    format!(
        "codeterm agent spawn {planner_agent}{model_flag} --parent {orch_tab_id} --role Planner"
    )
}

fn pair_retry_command(
    workspace_id: &str,
    orchestrator_agent: &str,
    orchestrator_model: Option<&str>,
    planner_agent: &str,
    planner_model: Option<&str>,
) -> String {
    let provider_spec = |provider: &str, model: Option<&str>| match model
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        Some(model) => format!("{provider}:{model}"),
        None => provider.to_string(),
    };
    format!(
        "codeterm orchestration new --orchestrator {} --planner {} --workspace {}",
        provider_spec(orchestrator_agent, orchestrator_model),
        provider_spec(planner_agent, planner_model),
        workspace_id
    )
}

fn derive_planner_name<'a>(
    existing_titles: impl IntoIterator<Item = &'a str>,
    orch_title: &str,
) -> String {
    let mut occupied: Vec<String> = existing_titles.into_iter().map(str::to_string).collect();
    occupied.push(orch_title.to_string());
    occupied.push("Planner".to_string());
    for _ in 0..8 {
        let candidate = crate::multiplexer::state::names::generate_unique_tab_name(
            occupied.iter().map(String::as_str),
        );
        if !candidate.to_ascii_lowercase().starts_with("planner") {
            return candidate;
        }
        occupied.push(candidate);
    }
    "Specialist".to_string()
}

struct ReservedPairTabs {
    orchestrator_tab_id: String,
    planner_tab_id: String,
    planner_name: String,
}

fn reserve_pair_tabs(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    workspace_id: &str,
    existing_orchestrator_tab_id: Option<&str>,
) -> Result<ReservedPairTabs, String> {
    let orchestrator_tab_id = match existing_orchestrator_tab_id {
        Some(tab_id) => mux.claim_existing_tab(workspace_id, tab_id)?,
        None => mux.create_tab(workspace_id)?,
    };
    let orchestrator_label = mux.display_label_for_tab(&orchestrator_tab_id);
    let existing_titles: Vec<String> = mux
        .get_workspace(workspace_id)
        .into_iter()
        .flat_map(|workspace| workspace.tabs.iter())
        .filter_map(|tab_id| mux.get_tab(tab_id).map(|tab| tab.title.clone()))
        .collect();
    let planner_name = derive_planner_name(
        existing_titles.iter().map(String::as_str),
        &orchestrator_label,
    );
    let planner_tab_id = mux.create_tab(workspace_id)?;
    mux.rename_tab_at_level(
        &planner_tab_id,
        &planner_name,
        crate::multiplexer::types::TitleLevel::User,
    )?;
    for tab_id in [&orchestrator_tab_id, &planner_tab_id] {
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.agent_booting = true;
            crate::service::boot_stage::apply_boot_stage(
                tab,
                codeterm_types::pty::BootStage::PtyStarting,
                None,
            );
        }
    }
    Ok(ReservedPairTabs {
        orchestrator_tab_id,
        planner_tab_id,
        planner_name,
    })
}

fn orchestrator_pair_followup_degraded(fix_command: &str, error: &str) -> String {
    format!(
        "Planner spawn failed ({error}). No plan exists yet and there is no live Planner specialist.\n\n{}\n\nAfter setting a goal, complete attachment with:\n  {fix_command}",
        crate::prompts::PEER_WAIVER_HINT
    )
}

fn pair_agent_state_allows_followup(
    booting: bool,
    boot_stage: Option<codeterm_types::pty::BootStage>,
    is_chat_backend: bool,
    tui_visible: bool,
) -> bool {
    !booting
        && boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
        && (is_chat_backend || tui_visible)
}

impl ServiceContext {
    fn pair_agent_live_now(&self, tab_id: &str, provider: &str) -> bool {
        let (booting, boot_stage, backing, pty_id) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return false;
            };
            (
                tab.agent_booting,
                tab.boot_stage,
                tab.backing.clone(),
                tab.pty_id.clone(),
            )
        };
        let is_chat_backend = matches!(
            backing,
            crate::multiplexer::types::TabBacking::ChatBackend { .. }
        );
        if is_chat_backend {
            return pair_agent_state_allows_followup(booting, boot_stage, true, false);
        }
        let Some(pty_id) = pty_id else {
            return false;
        };
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default();
        let provider_ready = self
            .agent_registry
            .with_provider(provider, |p| p.screen_has_tui(&screen));
        let tui_visible = crate::service::pty_io::provider_or_generic_tui(
            provider_ready,
            crate::pty::screen_state::screen_has_agent_tui(&screen),
        );
        pair_agent_state_allows_followup(booting, boot_stage, false, tui_visible)
    }

    async fn await_pair_agent_ready(
        &self,
        tab_id: &str,
        provider: &str,
        ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
        deadline: std::time::Instant,
    ) -> Result<(), String> {
        let now = std::time::Instant::now();
        let agent_deadline = deadline.min(now + PAIR_AGENT_READY_TIMEOUT);
        let is_chat_backend = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            matches!(
                &tab.backing,
                crate::multiplexer::types::TabBacking::ChatBackend { .. }
            )
        });
        if !is_chat_backend {
            let endpoint_remaining = deadline.saturating_duration_since(now);
            let remaining = agent_deadline.saturating_duration_since(now);
            let timeout_reason = if endpoint_remaining <= PAIR_AGENT_READY_TIMEOUT {
                format!(
                    "pair launch readiness deadline exceeded after {}s",
                    PAIR_API_READY_TIMEOUT.as_secs()
                )
            } else {
                format!(
                    "agent launch readiness deadline exceeded after {}s",
                    PAIR_AGENT_READY_TIMEOUT.as_secs()
                )
            };
            if remaining.is_zero() {
                return Err(timeout_reason);
            }
            match tokio::time::timeout(remaining, ready_rx).await {
                Ok(Ok(result)) => result?,
                Ok(Err(_)) => return Err("agent launch readiness channel closed".into()),
                Err(_) => return Err(timeout_reason),
            }
        }
        loop {
            if self.pair_agent_live_now(tab_id, provider) {
                return Ok(());
            }
            if std::time::Instant::now() >= agent_deadline {
                let failure = self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.boot_failure_reason.clone())
                    .unwrap_or_else(|| "agent TUI was not confirmed live".into());
                return Err(failure);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    fn deliver_pair_followup(
        &self,
        orchestrator_tab_id: &str,
        orchestrator_provider: &str,
        body: &str,
    ) -> Result<(), String> {
        if !self.pair_agent_live_now(orchestrator_tab_id, orchestrator_provider) {
            return Err(format!(
                "orchestrator Tab {orchestrator_tab_id} is not a confirmed live agent"
            ));
        }
        self.gateway_deliver(
            orchestrator_tab_id,
            MessageEnvelope::codeterm_reminder("orchestration-pair"),
            body,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        )
        .map(|_| ())
    }

    /// Bind a pending pair-spawned Planner when an Orchestrator creates its
    /// first plan. Returns true only when a pending tab was consumed.
    pub(crate) fn bind_pending_paired_planner(
        &self,
        plan_id: &str,
        owner: &OrchestratorRef,
        now_ms: u64,
    ) -> Result<bool, String> {
        let Some(orch_tab_id) = owner.tab_id.as_deref().filter(|p| !p.is_empty()) else {
            return Ok(false);
        };
        let planner_tab_id = {
            let mut mux = self.mux.write();
            let Some(orch) = mux.get_tab_mut(orch_tab_id) else {
                return Ok(false);
            };
            orch.pending_paired_planner_tab_id.take()
        };
        let Some(planner_tab_id) = planner_tab_id else {
            return Ok(false);
        };
        self.plan_store
            .set_planner_ref(
                plan_id,
                Some(PlannerRef::Tab {
                    tab_id: planner_tab_id.clone(),
                    session_id: None,
                    title: None,
                }),
                now_ms,
            )
            .map_err(|e| e.to_string())?;
        log::info!(
            "[orchestration-pair] bound pending planner {planner_tab_id} to first plan {plan_id} for orchestrator {orch_tab_id}"
        );
        Ok(true)
    }

    pub(crate) fn tab_has_pending_paired_planner(&self, tab_id: &str) -> bool {
        self.mux
            .read()
            .get_tab(tab_id)
            .and_then(|p| p.pending_paired_planner_tab_id.as_deref())
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    }

    /// Create an Orchestrator + Planner pair in one action. No plan is created
    /// at spawn time; the Orchestrator's first goal/plan create claims a new
    /// plan and binds the already-parented Planner. If the Planner spawn fails,
    /// no pending planner is recorded and the error names the exact recovery
    /// command — never a silent half-pair.
    pub async fn create_orchestration_pair(
        &self,
        params: OrchestrationPairParams,
    ) -> Result<OrchestrationPairResult, String> {
        if params.orchestrator_agent.trim().is_empty() {
            return Err("orchestrator_agent is required".into());
        }
        if params.planner_agent.trim().is_empty() {
            return Err("planner_agent is required".into());
        }

        // Leaf roles cannot create orchestration pairs.
        // Mirrors the child-spawn refusal in multiplexer::state::role.
        if let Some(ref spawner_id) = params.spawned_by_tab_id {
            if !spawner_id.is_empty() {
                let is_leaf = {
                    let mux = self.mux.write();
                    mux.get_tab(spawner_id)
                        .map(|p| {
                            matches!(
                                p.role,
                                Some(crate::multiplexer::types::AgentRole::Manager)
                                    | Some(crate::multiplexer::types::AgentRole::Worker)
                            )
                        })
                        .unwrap_or(false)
                };
                if is_leaf {
                    return Err(
                        "Manager and Worker Tabs cannot create orchestration pairs; \
                         leaf roles cannot spawn orchestrators"
                            .into(),
                    );
                }
            }
        }

        let readiness_deadline = std::time::Instant::now() + PAIR_API_READY_TIMEOUT;
        let workspace_id = {
            let mux = self.mux.write();
            params
                .workspace_id
                .clone()
                .or_else(|| mux.get_active_workspace().map(|w| w.id.clone()))
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or_else(|| "No workspace found".to_string())?
        };
        let project = self
            .mux
            .read()
            .get_workspace(&workspace_id)
            .map(|workspace| workspace.name.clone())
            .unwrap_or_else(|| workspace_id.clone());

        let orchestrator_model = params
            .orchestrator_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| crate::agent::model_catalog::resolve_default(&params.orchestrator_agent));
        let planner_model = params
            .planner_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| crate::agent::model_catalog::resolve_default(&params.planner_agent));
        let pair_fix_command = pair_retry_command(
            &workspace_id,
            &params.orchestrator_agent,
            orchestrator_model.as_deref(),
            &params.planner_agent,
            planner_model.as_deref(),
        );

        let reserved = {
            let mut mux = self.mux.write();
            reserve_pair_tabs(&mut mux, &workspace_id, params.existing_tab_id.as_deref())?
        };
        let orchestrator_label = self
            .mux
            .read()
            .display_label_for_tab(&reserved.orchestrator_tab_id);
        let orch_counterpart = crate::service::spawn_factory::Counterpart::new(
            "Orchestrator",
            &orchestrator_label,
            &reserved.orchestrator_tab_id,
        );
        let planner_counterpart = crate::service::spawn_factory::Counterpart::new(
            "Planner",
            &reserved.planner_name,
            &reserved.planner_tab_id,
        );
        let mut orchestrator_bootstrap = crate::service::spawn_factory::forming_identity(
            "Orchestrator",
            ORCHESTRATOR,
            project.clone(),
        );
        orchestrator_bootstrap.coordination =
            Some(crate::service::spawn_factory::introduction_body(
                &orch_counterpart,
                &planner_counterpart,
            ));
        let orch_params = SpawnAgentParams {
            agent: Some(params.orchestrator_agent.clone()),
            model: orchestrator_model.clone(),
            role: Some("Orchestrator".into()),
            can_spawn: Some(true),
            workspace_id: Some(workspace_id.clone()),
            spawned_by_tab_id: params.spawned_by_tab_id.clone(),
            task: None,
            manager_bootstrap: Some(orchestrator_bootstrap),
            existing_tab_id: Some(reserved.orchestrator_tab_id.clone()),
            skip_orchestrator_onboarding: Some(true),
            ..Default::default()
        };
        let (orch_ready_tx, orch_ready_rx) = tokio::sync::oneshot::channel();
        let orch_result = match self.spawn_agent(orch_params, Some(orch_ready_tx)).await {
            Ok(result) => result,
            Err(error) => {
                self.emit_boot_stage(
                    &reserved.orchestrator_tab_id,
                    codeterm_types::pty::BootStage::SpawnFailed,
                    Some(&error),
                );
                if let Some(tab) = self.mux.write().get_tab_mut(&reserved.orchestrator_tab_id) {
                    tab.agent_booting = false;
                }
                let _ = self.mux.write().close_tab(&reserved.planner_tab_id);
                self.emit_state_changed();
                return Err(format!(
                    "Failed to spawn Orchestrator ({error}); no plan was created. Retry with: {pair_fix_command}"
                ));
            }
        };
        let orch_tab_id = orch_result.tab_id.clone();

        let mut planner_bootstrap =
            crate::service::spawn_factory::forming_identity("Planner", PLANNER, project);
        planner_bootstrap.coordination = Some(crate::service::spawn_factory::introduction_body(
            &planner_counterpart,
            &orch_counterpart,
        ));
        let planner_params = SpawnAgentParams {
            agent: Some(params.planner_agent.clone()),
            model: planner_model.clone(),
            title: Some(reserved.planner_name.clone()),
            role: Some("Manager".into()),
            role_profile: Some("planner".into()),
            parent_tab_id: Some(orch_tab_id.clone()),
            plan_id: None,
            workspace_id: Some(workspace_id.clone()),
            spawned_by_tab_id: Some(orch_tab_id.clone()),
            task: None,
            manager_bootstrap: Some(planner_bootstrap),
            existing_tab_id: Some(reserved.planner_tab_id.clone()),
            ..Default::default()
        };

        let (planner_ready_tx, planner_ready_rx) = tokio::sync::oneshot::channel();
        match self
            .spawn_agent(planner_params, Some(planner_ready_tx))
            .await
        {
            Ok(planner_result) => {
                let planner_tab_id = planner_result.tab_id.clone();
                if let Some(tab) = self.mux.write().get_tab_mut(&orch_tab_id) {
                    tab.pending_paired_planner_tab_id = Some(planner_tab_id.clone());
                }
                let (orch_ready, planner_ready) = tokio::join!(
                    self.await_pair_agent_ready(
                        &orch_tab_id,
                        &params.orchestrator_agent,
                        orch_ready_rx,
                        readiness_deadline,
                    ),
                    self.await_pair_agent_ready(
                        &planner_tab_id,
                        &params.planner_agent,
                        planner_ready_rx,
                        readiness_deadline,
                    )
                );
                let warning = match (orch_ready, planner_ready) {
                    (Ok(()), Ok(())) => None,
                    (orch_status, planner_status) => {
                        let orch_error = orch_status.err();
                        let planner_error = planner_status.err();
                        for (tab_id, error) in [
                            (&orch_tab_id, orch_error.as_deref()),
                            (&planner_tab_id, planner_error.as_deref()),
                        ] {
                            if let Some(error) = error {
                                self.emit_boot_stage(
                                    tab_id,
                                    codeterm_types::pty::BootStage::SpawnFailed,
                                    Some(error),
                                );
                                if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
                                    tab.agent_booting = false;
                                }
                            }
                        }
                        let fix_command = degraded_fix_command(
                            &orch_tab_id,
                            &params.planner_agent,
                            planner_model.as_deref(),
                        );
                        if let Some(ref planner_error) = planner_error {
                            let followup =
                                orchestrator_pair_followup_degraded(&fix_command, planner_error);
                            if let Err(notify_err) = self.deliver_pair_followup(
                                &orch_tab_id,
                                &params.orchestrator_agent,
                                &followup,
                            ) {
                                log::warn!(
                                    "[orchestration-pair] failed to notify orchestrator {orch_tab_id} of degraded pairing: {notify_err}"
                                );
                            }
                        }
                        let mut parts = Vec::new();
                        if let Some(error) = orch_error {
                            parts
                                .push(format!("Orchestrator readiness was not confirmed: {error}"));
                        }
                        if let Some(error) = planner_error {
                            parts.push(format!("Planner readiness was not confirmed: {error}"));
                        }
                        parts.push(format!(
                            "Both tabs were kept open. If the Planner is actually missing after the first goal, complete attachment with: {fix_command}"
                        ));
                        Some(parts.join(" "))
                    }
                };
                // Activate server-side: the client continuation may be gone by
                // the time the pair lands (page reload); the daemon owns focus.
                {
                    let mut mux = self.mux.write();
                    let orch_tab = mux
                        .get_workspace(&workspace_id)
                        .and_then(|ws| ws.tabs.iter().find(|tab_id| *tab_id == &orch_tab_id))
                        .cloned();
                    let _ = mux.set_active_workspace(&workspace_id);
                    if let Some(tab_id) = orch_tab {
                        let _ = mux.set_active_tab(&workspace_id, &tab_id);
                    }
                }
                let _ = self.focus_tab(&orch_tab_id);
                self.emit_state_changed();
                // final count is the authoritative one for the nudge.
                let budget_notice = self.workspace_tab_budget_notice(&workspace_id);
                Ok(OrchestrationPairResult {
                    plan_id: None,
                    orchestrator_tab_id: orch_tab_id,
                    orchestrator_model,
                    planner_tab_id,
                    planner_model,
                    warning: warning.or(orch_result.warning).or(planner_result.warning),
                    budget_notice,
                })
            }
            Err(e) => {
                self.emit_boot_stage(
                    &reserved.planner_tab_id,
                    codeterm_types::pty::BootStage::SpawnFailed,
                    Some(&e),
                );
                if let Some(tab) = self.mux.write().get_tab_mut(&reserved.planner_tab_id) {
                    tab.agent_booting = false;
                }
                let fix_command = degraded_fix_command(
                    &orch_tab_id,
                    &params.planner_agent,
                    planner_model.as_deref(),
                );
                let followup = orchestrator_pair_followup_degraded(&fix_command, &e);
                if let Err(notify_err) =
                    self.deliver_pair_followup(&orch_tab_id, &params.orchestrator_agent, &followup)
                {
                    log::warn!(
                        "[orchestration-pair] failed to notify orchestrator {orch_tab_id} of degraded pairing: {notify_err}"
                    );
                }
                Err(format!(
                    "Orchestrator spawned (Tab {orch_tab_id}) but Planner spawn failed: {e}. No plan was created. Complete the pair after first goal with: {fix_command}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::PlannerRef;
    use crate::service::plan::PlanStore;

    fn store_for_test() -> (tempfile::TempDir, PlanStore) {
        let root = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(root.path());
        (root, store)
    }

    #[test]
    fn pair_spawn_creates_no_plan_until_first_goal() {
        let (_root, store) = store_for_test();
        assert!(
            store.list_index().is_empty(),
            "pair spawn must not create a placeholder plan"
        );

        let plan = store
            .create_plan_for_team(
                "First goal".into(),
                _root.path().display().to_string(),
                OrchestratorRef::from_tab("orch-1"),
                "team-orch-1".into(),
                Vec::new(),
                1,
            )
            .unwrap();
        store
            .set_planner_ref(
                &plan.id,
                Some(PlannerRef::Tab {
                    tab_id: "planner-1".into(),
                    session_id: None,
                    title: None,
                }),
                3,
            )
            .unwrap();

        let plans = store.list_index();
        assert_eq!(plans.len(), 1, "first goal creates exactly one plan");
        let bound = store.get(&plan.id).unwrap();
        assert_eq!(bound.owner.tab_id.as_deref(), Some("orch-1"));
        assert_eq!(
            bound.planner,
            Some(PlannerRef::Tab {
                tab_id: "planner-1".into(),
                session_id: None,
                title: None,
            })
        );
    }

    #[test]
    fn degraded_path_creates_no_plan_when_planner_spawn_fails() {
        let (_root, store) = store_for_test();
        assert!(
            store.list_index().is_empty(),
            "degraded pair spawn must not leave an unowned empty plan"
        );
    }

    #[test]
    fn degraded_fix_command_includes_model_when_present() {
        let cmd = degraded_fix_command("orch-1", "codex", Some("gpt-5"));
        assert!(cmd.contains("agent spawn codex"));
        assert!(cmd.contains("--model gpt-5"));
        assert!(cmd.contains("--parent orch-1"));
        assert!(cmd.contains("--role Planner"));
        assert!(!cmd.contains("--plan"));
    }

    #[test]
    fn degraded_fix_command_omits_model_flag_when_absent() {
        let cmd = degraded_fix_command("orch-1", "codex", None);
        assert!(!cmd.contains("--model"));
    }

    #[test]
    fn pair_retry_command_preserves_provider_models_and_workspace() {
        let cmd = pair_retry_command("ws-1", "claude", Some("opus"), "codex", Some("gpt-5"));
        assert_eq!(
            cmd,
            "codeterm orchestration new --orchestrator claude:opus --planner codex:gpt-5 --workspace ws-1"
        );
    }

    #[test]
    fn pair_readiness_budgets_cap_each_agent_and_the_whole_request() {
        assert_eq!(PAIR_AGENT_READY_TIMEOUT, std::time::Duration::from_secs(45));
        assert_eq!(PAIR_API_READY_TIMEOUT, std::time::Duration::from_secs(75));
        assert!(PAIR_AGENT_READY_TIMEOUT < PAIR_API_READY_TIMEOUT);
        assert!(PAIR_API_READY_TIMEOUT < PAIR_AGENT_READY_TIMEOUT * 2);
    }

    #[test]
    fn pair_followup_refuses_bare_shell_and_booting_agent() {
        use codeterm_types::pty::BootStage;

        assert!(!pair_agent_state_allows_followup(
            false,
            Some(BootStage::SpawnFailed),
            false,
            false
        ));
        assert!(!pair_agent_state_allows_followup(
            false,
            Some(BootStage::AgentReady),
            false,
            false
        ));
        assert!(!pair_agent_state_allows_followup(
            true,
            Some(BootStage::AgentReady),
            false,
            true
        ));
        assert!(pair_agent_state_allows_followup(
            false,
            Some(BootStage::AgentReady),
            false,
            true
        ));
    }

    #[test]
    fn planner_name_is_distinct_from_orchestrator_and_never_the_role_word() {
        for _ in 0..64 {
            let name = derive_planner_name(["Kepler", "Main"], "Kepler");
            assert_ne!(
                name, "Kepler",
                "planner never copies the orchestrator's name"
            );
            assert_ne!(name, "Planner", "planner is never the literal role word");
            assert!(
                !name.to_ascii_lowercase().starts_with("planner"),
                "planner name never even STARTS with the role word (got '{name}')"
            );
            assert!(!name.trim().is_empty(), "planner always gets a real name");
        }
    }

    #[test]
    fn planner_name_avoids_all_existing_workspace_titles() {
        let existing = ["Euler", "Noether", "Bohr"];
        for _ in 0..64 {
            let name = derive_planner_name(existing, "Euler");
            assert!(
                !existing.contains(&name.as_str()),
                "planner name '{name}' collided with an existing workspace tab"
            );
            assert_ne!(name, "Planner");
        }
    }

    #[test]
    fn pair_tabs_are_reserved_before_either_agent_launches() {
        let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let reserved = reserve_pair_tabs(&mut mux, &workspace_id, None).unwrap();

        assert_ne!(reserved.orchestrator_tab_id, reserved.planner_tab_id);
        assert!(mux.get_tab(&reserved.orchestrator_tab_id).is_some());
        assert!(mux.get_tab(&reserved.planner_tab_id).is_some());
        for tab_id in [&reserved.orchestrator_tab_id, &reserved.planner_tab_id] {
            let tab = mux.get_tab(tab_id).unwrap();
            assert!(tab.agent_booting, "reserved Tab must announce booting");
            assert_eq!(
                tab.boot_stage,
                Some(codeterm_types::pty::BootStage::PtyStarting),
                "reserved Tab must never expose a blank chat before launch"
            );
        }
        assert_eq!(
            mux.display_label_for_tab(&reserved.planner_tab_id),
            reserved.planner_name
        );
        assert!(mux
            .claim_existing_tab(&workspace_id, &reserved.orchestrator_tab_id)
            .is_ok());
        assert!(mux
            .claim_existing_tab(&workspace_id, &reserved.planner_tab_id)
            .is_ok());
    }

    #[test]
    fn degraded_followup_names_exact_fix_command_and_waiver_hint() {
        let fix = degraded_fix_command("orch-1", "codex", None);
        let msg = orchestrator_pair_followup_degraded(&fix, "spawn timed out");
        assert!(msg.contains("spawn timed out"));
        assert!(msg.contains(&fix));
        assert!(msg.contains(crate::prompts::PEER_WAIVER_HINT));
    }
}
