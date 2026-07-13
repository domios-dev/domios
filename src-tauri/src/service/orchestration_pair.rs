//! Paired creation (task_7e5f9354): spawn an Orchestrator + Planner together,
//! linked from birth by pane hierarchy; the plan is created lazily on the
//! Orchestrator's first goal/plan create. Builds entirely on the
//! planner-attachment primitives already wired by the peer-foundation
//! (`crate::plan::peer`) and Lane 1 (`crate::service::plan::peer_mandate`,
//! Manager + planner binding in `Self::spawn_agent`) - this module
//! only sequences the two spawns and records the pending pair.

use serde::{Deserialize, Serialize};

use crate::plan::{OrchestratorRef, PlannerRef};
use crate::service::messenger::{MessageEnvelope, Messenger};
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
    pub title: Option<String>,
    pub spawned_by_pane_id: Option<String>,
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
    pub orchestrator_pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator_model: Option<String>,
    pub planner_pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Advisory pane-budget nudge (task_4d649dfd) for the workspace AFTER both
    /// panes land — a pair adds two, so its count is the authoritative one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_notice: Option<crate::service::pane_budget::PaneBudgetNotice>,
}

fn epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Default plan title for a pair created without an explicit one. Carries the
/// workspace name + a timestamp so many pairs stay distinguishable instead of
/// piling up as identical bare "Orchestration" rows.
fn default_orchestration_plan_title(workspace_name: &str, now_ms: u64) -> String {
    let stamp = chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let name = workspace_name.trim();
    match (name.is_empty(), stamp.is_empty()) {
        (true, true) => "Orchestration".to_string(),
        (true, false) => format!("Orchestration — {stamp}"),
        (false, true) => format!("Orchestration — {name}"),
        (false, false) => format!("Orchestration — {name} {stamp}"),
    }
}

/// Orchestrator's first task. Unlike the generic promotion mandate
/// ([`crate::prompts::peer_pairing_mandate_full`]), this flow has ALREADY
/// spawned the Planner synchronously by the time the Orchestrator reads this
/// — the generic "spawn it if not live" instruction previously caused a live
/// model to race ahead and spawn a redundant, mis-parented Planner of its own
/// (observed live: `codeterm agent spawn ... --role Planner` with no
/// `--parent`, immediately after the real one was already bound) before the
/// async "Planner is live" follow-up could reach it. Tell it plainly not to.
fn orchestrator_pair_task() -> String {
    format!(
        "You are the Orchestrator for a fresh paired session. No plan exists yet. A Planner \
         specialist is being spawned and parented to your pane automatically as part of this \
         request — do NOT run `codeterm agent spawn` yourself to create it; wait for the \
         follow-up message naming its live pane id.\n\n\
         First set your GOAL with CodeTerm (`codeterm plan goal set` or create the plan first); \
         that first goal/plan create lazily creates and claims the plan, then binds the paired \
         Planner. Until the user states a goal, WAIT.\n\n\
         ## Planner attachment\n\n\
         The plan will have an attached Planner specialist after the first goal/plan create. \
         The Orchestrator is the single authority for structure, execution, and acceptance. \
         Do not treat the Planner as a co-owner.\n\n\
         {}\n{}",
        crate::prompts::PEER_PLANNER_CHARTER_LINE,
        crate::prompts::PEER_ORCH_DEFERENCE_LINE,
    )
}

/// Follow-up delivered to the Orchestrator once the Planner spawn succeeds —
/// the first point both real pane ids are known.
fn orchestrator_pair_followup_ready(planner_pane_id: &str) -> String {
    format!(
        "Your Planner specialist is live: pane {planner_pane_id}. No plan exists yet; your first goal/plan create will claim the plan and bind this Planner. {}",
        crate::prompts::PEER_ORCH_DEFERENCE_LINE
    )
}

/// The exact command to finish a degraded pair by hand, using the real
/// requested provider/model rather than a hardcoded default.
fn degraded_fix_command(
    orch_pane_id: &str,
    planner_agent: &str,
    planner_model: Option<&str>,
) -> String {
    let model_flag = planner_model
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!(" --model {m}"))
        .unwrap_or_default();
    format!(
        "codeterm agent spawn {planner_agent}{model_flag} --parent {orch_pane_id} --role Planner"
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
    fn pair_agent_live_now(&self, pane_id: &str, provider: &str) -> bool {
        let (booting, boot_stage, backing, pty_id) = {
            let mux = self.mux.read();
            let Some(pane) = mux.get_pane(pane_id) else {
                return false;
            };
            (
                pane.agent_booting,
                pane.boot_stage,
                pane.backing.clone(),
                pane.pty_id.clone(),
            )
        };
        let is_chat_backend = matches!(
            backing,
            crate::multiplexer::types::PaneBacking::ChatBackend { .. }
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
        let tui_visible = crate::pty::screen_state::screen_has_agent_tui(&screen)
            || self
                .agent_registry
                .with_provider(provider, |p| p.screen_has_tui(&screen))
                .unwrap_or(false);
        pair_agent_state_allows_followup(booting, boot_stage, false, tui_visible)
    }

    async fn await_pair_agent_ready(
        &self,
        pane_id: &str,
        provider: &str,
        ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
        deadline: std::time::Instant,
    ) -> Result<(), String> {
        let now = std::time::Instant::now();
        let agent_deadline = deadline.min(now + PAIR_AGENT_READY_TIMEOUT);
        let is_chat_backend = self.mux.read().get_pane(pane_id).is_some_and(|pane| {
            matches!(
                &pane.backing,
                crate::multiplexer::types::PaneBacking::ChatBackend { .. }
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
            if self.pair_agent_live_now(pane_id, provider) {
                return Ok(());
            }
            if std::time::Instant::now() >= agent_deadline {
                let failure = self
                    .mux
                    .read()
                    .get_pane(pane_id)
                    .and_then(|pane| pane.boot_failure_reason.clone())
                    .unwrap_or_else(|| "agent TUI was not confirmed live".into());
                return Err(failure);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    fn deliver_pair_followup(
        &self,
        orchestrator_pane_id: &str,
        orchestrator_provider: &str,
        body: &str,
    ) -> Result<(), String> {
        if !self.pair_agent_live_now(orchestrator_pane_id, orchestrator_provider) {
            return Err(format!(
                "orchestrator pane {orchestrator_pane_id} is not a confirmed live agent"
            ));
        }
        Messenger::deliver(
            self,
            orchestrator_pane_id,
            MessageEnvelope::codeterm_reminder("orchestration-pair"),
            body,
        )
        .map(|_| ())
    }

    /// Bind a pending pair-spawned Planner when an Orchestrator creates its
    /// first plan. Returns true only when a pending pane was consumed.
    pub(crate) fn bind_pending_paired_planner(
        &self,
        plan_id: &str,
        owner: &OrchestratorRef,
        now_ms: u64,
    ) -> Result<bool, String> {
        let Some(orch_pane_id) = owner.pane_id.as_deref().filter(|p| !p.is_empty()) else {
            return Ok(false);
        };
        let planner_pane_id = {
            let mut mux = self.mux.write();
            let Some(orch) = mux.get_pane_mut(orch_pane_id) else {
                return Ok(false);
            };
            orch.pending_paired_planner_pane_id.take()
        };
        let Some(planner_pane_id) = planner_pane_id else {
            return Ok(false);
        };
        self.plan_store
            .set_planner_ref(
                plan_id,
                Some(PlannerRef::Pane {
                    pane_id: planner_pane_id.clone(),
                    session_id: None,
                    title: None,
                }),
                now_ms,
            )
            .map_err(|e| e.to_string())?;
        log::info!(
            "[orchestration-pair] bound pending planner {planner_pane_id} to first plan {plan_id} for orchestrator {orch_pane_id}"
        );
        Ok(true)
    }

    pub(crate) fn pane_has_pending_paired_planner(&self, pane_id: &str) -> bool {
        self.mux
            .read()
            .get_pane(pane_id)
            .and_then(|p| p.pending_paired_planner_pane_id.as_deref())
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
        if let Some(ref spawner_id) = params.spawned_by_pane_id {
            if !spawner_id.is_empty() {
                let is_leaf = {
                    let mux = self.mux.write();
                    mux.get_pane(spawner_id)
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
                        "Manager and Worker panes cannot create orchestration pairs; \
                         leaf roles cannot spawn orchestrators"
                            .into(),
                    );
                }
            }
        }

        let now_ms = epoch_ms();
        let readiness_deadline = std::time::Instant::now() + PAIR_API_READY_TIMEOUT;
        let (workspace_id, project_path, workspace_name) = {
            let mux = self.mux.write();
            let ws_id = params
                .workspace_id
                .clone()
                .or_else(|| mux.get_active_workspace().map(|w| w.id.clone()))
                .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
                .ok_or_else(|| "No workspace found".to_string())?;
            let cwd = mux
                .get_workspace(&ws_id)
                .and_then(|ws| ws.cwd.clone())
                .unwrap_or_default();
            let name = mux
                .get_workspace(&ws_id)
                .map(|ws| ws.name.clone())
                .unwrap_or_default();
            (ws_id, cwd, name)
        };

        let _title = params
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| default_orchestration_plan_title(&workspace_name, now_ms));
        let _project_path = project_path;

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

        let orch_params = SpawnAgentParams {
            agent: Some(params.orchestrator_agent.clone()),
            model: orchestrator_model.clone(),
            role: Some("Orchestrator".into()),
            can_spawn: Some(true),
            workspace_id: Some(workspace_id.clone()),
            spawned_by_pane_id: params.spawned_by_pane_id.clone(),
            task: Some(orchestrator_pair_task()),
            existing_tab_id: params.existing_tab_id.clone(),
            skip_orchestrator_onboarding: Some(true),
            ..Default::default()
        };
        let (orch_ready_tx, orch_ready_rx) = tokio::sync::oneshot::channel();
        let orch_result = self
            .spawn_agent(orch_params, Some(orch_ready_tx))
            .await
            .map_err(|e| {
                format!(
                    "Failed to spawn Orchestrator ({e}); no plan was created. Retry with: {pair_fix_command}"
                )
            })?;
        let orch_pane_id = orch_result.pane_id.clone();

        let planner_name = {
            let mux = self.mux.read();
            let existing: Vec<String> = mux
                .get_workspace(&workspace_id)
                .map(|ws| {
                    ws.tabs
                        .iter()
                        .filter_map(|t| mux.get_pane(&t.pane_id).map(|p| p.title.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let orch_title = mux.display_label_for_pane(&orch_pane_id);
            derive_planner_name(existing.iter().map(String::as_str), &orch_title)
        };

        let planner_params = SpawnAgentParams {
            agent: Some(params.planner_agent.clone()),
            model: planner_model.clone(),
            title: Some(planner_name),
            role: Some("Manager".into()),
            role_profile: Some("planner".into()),
            parent_pane_id: Some(orch_pane_id.clone()),
            plan_id: None,
            workspace_id: Some(workspace_id.clone()),
            spawned_by_pane_id: Some(orch_pane_id.clone()),
            task: Some(crate::prompts::planner_onboarding_task_no_plan(
                &self.mux.read().display_label_for_pane(&orch_pane_id),
                &orch_pane_id,
            )),
            ..Default::default()
        };

        let (planner_ready_tx, planner_ready_rx) = tokio::sync::oneshot::channel();
        match self
            .spawn_agent(planner_params, Some(planner_ready_tx))
            .await
        {
            Ok(planner_result) => {
                let planner_pane_id = planner_result.pane_id.clone();
                let planner_label = self
                    .agent_registry
                    .with_provider(&params.planner_agent, |p| p.display_name().to_string())
                    .unwrap_or_else(|| params.planner_agent.clone());
                if let Some(pane) = self.mux.write().get_pane_mut(&orch_pane_id) {
                    pane.pending_greeting =
                        Some(crate::prompts::peer_orch_greeting(&planner_label));
                }
                // boots into silence — names its Orchestrator + charter.
                let orch_label = self.mux.read().display_label_for_pane(&orch_pane_id);
                if let Some(pane) = self.mux.write().get_pane_mut(&planner_pane_id) {
                    pane.pending_greeting = Some(crate::prompts::peer_planner_greeting(
                        &orch_label,
                        &orch_pane_id,
                    ));
                }
                if let Some(pane) = self.mux.write().get_pane_mut(&orch_pane_id) {
                    pane.pending_paired_planner_pane_id = Some(planner_pane_id.clone());
                }
                let (orch_ready, planner_ready) = tokio::join!(
                    self.await_pair_agent_ready(
                        &orch_pane_id,
                        &params.orchestrator_agent,
                        orch_ready_rx,
                        readiness_deadline,
                    ),
                    self.await_pair_agent_ready(
                        &planner_pane_id,
                        &params.planner_agent,
                        planner_ready_rx,
                        readiness_deadline,
                    )
                );
                let warning = match (orch_ready, planner_ready) {
                    (Ok(()), Ok(())) => {
                        let followup = orchestrator_pair_followup_ready(&planner_pane_id);
                        if let Err(error) = self.deliver_pair_followup(
                            &orch_pane_id,
                            &params.orchestrator_agent,
                            &followup,
                        ) {
                            Some(format!(
                                "Pair launched, but follow-up delivery to Orchestrator pane {orch_pane_id} failed: {error}"
                            ))
                        } else {
                            None
                        }
                    }
                    (orch_status, planner_status) => {
                        let orch_error = orch_status.err();
                        let planner_error = planner_status.err();
                        let fix_command = degraded_fix_command(
                            &orch_pane_id,
                            &params.planner_agent,
                            planner_model.as_deref(),
                        );
                        if let Some(ref planner_error) = planner_error {
                            let followup =
                                orchestrator_pair_followup_degraded(&fix_command, planner_error);
                            if let Err(notify_err) = self.deliver_pair_followup(
                                &orch_pane_id,
                                &params.orchestrator_agent,
                                &followup,
                            ) {
                                log::warn!(
                                    "[orchestration-pair] failed to notify orchestrator {orch_pane_id} of degraded pairing: {notify_err}"
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
                        .and_then(|ws| ws.tabs.iter().find(|t| t.pane_id == orch_pane_id))
                        .map(|t| t.id.clone());
                    let _ = mux.set_active_workspace(&workspace_id);
                    if let Some(tab_id) = orch_tab {
                        let _ = mux.set_active_tab(&workspace_id, &tab_id);
                    }
                }
                let _ = self.focus_pane(&orch_pane_id);
                self.emit_state_changed();
                // final count is the authoritative one for the nudge.
                let budget_notice = self.workspace_pane_budget_notice(&workspace_id);
                Ok(OrchestrationPairResult {
                    plan_id: None,
                    orchestrator_pane_id: orch_pane_id,
                    orchestrator_model,
                    planner_pane_id,
                    planner_model,
                    warning: warning.or(orch_result.warning).or(planner_result.warning),
                    budget_notice,
                })
            }
            Err(e) => {
                let fix_command = degraded_fix_command(
                    &orch_pane_id,
                    &params.planner_agent,
                    planner_model.as_deref(),
                );
                let followup = orchestrator_pair_followup_degraded(&fix_command, &e);
                if let Err(notify_err) =
                    self.deliver_pair_followup(&orch_pane_id, &params.orchestrator_agent, &followup)
                {
                    log::warn!(
                        "[orchestration-pair] failed to notify orchestrator {orch_pane_id} of degraded pairing: {notify_err}"
                    );
                }
                Err(format!(
                    "Orchestrator spawned (pane {orch_pane_id}) but Planner spawn failed: {e}. No plan was created. Complete the pair after first goal with: {fix_command}"
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
    fn default_plan_title_disambiguates_by_workspace_and_stamp() {
        let t = default_orchestration_plan_title("my-ws", 1_609_459_200_000);
        assert!(t.contains("my-ws"), "title carries the workspace name: {t}");
        assert!(t.contains("2021-01-01"), "title carries a date stamp: {t}");
        assert_ne!(t, "Orchestration", "never the bare piled-up default");
        // Empty workspace still avoids the bare string when a stamp exists.
        let no_ws = default_orchestration_plan_title("  ", 1_609_459_200_000);
        assert!(
            no_ws.starts_with("Orchestration — "),
            "stamped even without a name: {no_ws}"
        );
    }

    #[test]
    fn pair_spawn_creates_no_plan_until_first_goal() {
        let (_root, store) = store_for_test();
        assert!(
            store.list_index().is_empty(),
            "pair spawn must not create a placeholder plan"
        );

        let plan = store
            .create_plan(
                "First goal".into(),
                _root.path().display().to_string(),
                OrchestratorRef::from_pane("orch-1"),
                Vec::new(),
                1,
            )
            .unwrap();
        store
            .set_planner_ref(
                &plan.id,
                Some(PlannerRef::Pane {
                    pane_id: "planner-1".into(),
                    session_id: None,
                    title: None,
                }),
                3,
            )
            .unwrap();

        let plans = store.list_index();
        assert_eq!(plans.len(), 1, "first goal creates exactly one plan");
        let bound = store.get(&plan.id).unwrap();
        assert_eq!(bound.owner.pane_id.as_deref(), Some("orch-1"));
        assert_eq!(
            bound.planner,
            Some(PlannerRef::Pane {
                pane_id: "planner-1".into(),
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
    fn orchestrator_task_has_no_plan_yet_wording_and_reuses_peer_constants() {
        let task = orchestrator_pair_task();
        assert!(task.contains("No plan exists yet"));
        assert!(task.contains("first goal/plan create"));
        assert!(task.contains(crate::prompts::PEER_PLANNER_CHARTER_LINE));
        assert!(task.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
        assert!(task.contains("will have an attached Planner specialist"));
    }

    /// Regression for a live-dogfood finding: the pair-creation flow always
    /// spawns the Planner synchronously before the Orchestrator's task is
    /// even delivered, so telling it to spawn one "if not live" raced a real
    /// model into creating a redundant, mis-parented extra Worker pane.
    #[test]
    fn orchestrator_task_never_tells_model_to_spawn_its_own_planner() {
        let task = orchestrator_pair_task();
        assert!(
            !task.contains("--role Planner --plan"),
            "task must never hand the model a copy-pasteable Planner spawn command: {task}"
        );
        assert!(task.contains("do NOT run"));
    }

    #[test]
    fn followup_ready_names_real_planner_pane_and_no_plan_yet() {
        let msg = orchestrator_pair_followup_ready("pane-planner-1");
        assert!(msg.contains("pane-planner-1"));
        assert!(msg.contains("No plan exists yet"));
        assert!(msg.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
        assert!(msg.contains("Planner specialist is live"));
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
    fn degraded_followup_names_exact_fix_command_and_waiver_hint() {
        let fix = degraded_fix_command("orch-1", "codex", None);
        let msg = orchestrator_pair_followup_degraded(&fix, "spawn timed out");
        assert!(msg.contains("spawn timed out"));
        assert!(msg.contains(&fix));
        assert!(msg.contains(crate::prompts::PEER_WAIVER_HINT));
    }
}
