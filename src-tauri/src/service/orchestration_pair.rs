//! Paired creation (task_7e5f9354): spawn an Orchestrator + Planner together,
//! linked from birth via a freshly-created plan. Builds entirely on the
//! planner-attachment primitives already wired by the peer-foundation
//! (`crate::plan::peer`) and Lane 1 (`crate::service::plan::peer_mandate`,
//! Manager + planner binding in `Self::spawn_agent`) - this module
//! only sequences the two spawns + the plan create through one call.

use serde::{Deserialize, Serialize};

use crate::api::events::emit_everywhere;
use crate::plan::{MilestoneKind, OrchestratorRef};
use crate::service::messenger::{MessageEnvelope, Messenger};
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::ServiceContext;

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
    pub plan_id: String,
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
fn orchestrator_pair_task(plan_id: &str) -> String {
    format!(
        "You are the Orchestrator for plan {plan_id}. A Planner specialist is being spawned for \
         this plan automatically as part of this request — do NOT run `codeterm agent spawn` \
         yourself to create it; wait for the follow-up message naming its live pane id.\n\n\
         ## Planner attachment\n\n\
         This plan has an attached Planner specialist. \
         The Orchestrator is the single authority for structure, execution, and acceptance. \
         Do not treat the Planner as a co-owner.\n\n\
         {}\n{}",
        crate::prompts::PEER_PLANNER_CHARTER_LINE,
        crate::prompts::PEER_ORCH_DEFERENCE_LINE,
    )
}

/// Follow-up delivered to the Orchestrator once the Planner spawn succeeds —
/// the first point both real pane ids are known.
fn orchestrator_pair_followup_ready(plan_id: &str, planner_pane_id: &str) -> String {
    format!(
        "Your Planner specialist is live: pane {planner_pane_id} for plan {plan_id}. {}",
        crate::prompts::PEER_ORCH_DEFERENCE_LINE
    )
}

/// The exact command to finish a degraded pair by hand, using the real
/// requested provider/model rather than a hardcoded default.
fn degraded_fix_command(
    orch_pane_id: &str,
    plan_id: &str,
    planner_agent: &str,
    planner_model: Option<&str>,
) -> String {
    let model_flag = planner_model
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!(" --model {m}"))
        .unwrap_or_default();
    format!(
        "codeterm agent spawn {planner_agent}{model_flag} --parent {orch_pane_id} --role Planner --plan {plan_id}"
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
        "Planner spawn failed ({error}). This plan has no live Planner specialist yet.\n\n{}\n\nComplete attachment with:\n  {fix_command}",
        crate::prompts::PEER_WAIVER_HINT
    )
}

impl ServiceContext {
    /// Create an Orchestrator + Planner pair in one action: a fresh plan,
    /// an Orchestrator pane that claims it, and a Planner pane bound to it
    /// via the existing `role_profile=planner` + `plan_id` spawn contract
    /// (`Self::spawn_agent`). If the Planner spawn fails, the plan's
    /// `planner` ref is left unbound (surfaced by the doctor pairing ladder)
    /// and the error names the exact recovery command — never a silent
    /// half-pair.
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
                    return Err("Manager and Worker panes cannot create orchestration pairs; \
                         leaf roles cannot spawn orchestrators"
                        .into());
                }
            }
        }

        let now_ms = epoch_ms();
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

        // pairs don't pile up as indistinguishable bare "Orchestration" rows.
        let title = params
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| default_orchestration_plan_title(&workspace_name, now_ms));
        let plan = self
            .plan_store
            .create_plan(
                title,
                project_path,
                OrchestratorRef::default(),
                Vec::<MilestoneKind>::new(),
                now_ms,
            )
            .map_err(|e| e.to_string())?;
        let plan_id = plan.id.clone();

        let orchestrator_model = params
            .orchestrator_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| crate::agent::model_catalog::resolve_default(&params.orchestrator_agent));

        let orch_params = SpawnAgentParams {
            agent: Some(params.orchestrator_agent.clone()),
            model: orchestrator_model.clone(),
            role: Some("Orchestrator".into()),
            can_spawn: Some(true),
            workspace_id: Some(workspace_id.clone()),
            spawned_by_pane_id: params.spawned_by_pane_id.clone(),
            task: Some(orchestrator_pair_task(&plan_id)),
            existing_tab_id: params.existing_tab_id.clone(),
            skip_orchestrator_onboarding: Some(true),
            ..Default::default()
        };
        let orch_result = self.spawn_agent(orch_params, None).await.map_err(|e| {
            format!(
                "Failed to spawn Orchestrator ({e}); plan {plan_id} was created but has no owner. Retry `orchestration new`."
            )
        })?;
        let orch_pane_id = orch_result.pane_id.clone();

        if let Err(e) =
            self.plan_store
                .claim(&plan_id, OrchestratorRef::from_pane(&orch_pane_id), now_ms)
        {
            log::warn!(
                "[orchestration-pair] failed to claim plan {plan_id} for orchestrator {orch_pane_id}: {e}"
            );
        }
        emit_everywhere(
            &self.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );

        let planner_model = params
            .planner_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| crate::agent::model_catalog::resolve_default(&params.planner_agent));

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
            plan_id: Some(plan_id.clone()),
            workspace_id: Some(workspace_id.clone()),
            spawned_by_pane_id: Some(orch_pane_id.clone()),
            ..Default::default()
        };

        match self.spawn_agent(planner_params, None).await {
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
                let followup = orchestrator_pair_followup_ready(&plan_id, &planner_pane_id);
                let envelope = MessageEnvelope::reminder("CodeTerm", "orchestration-pair");
                if let Err(e) = Messenger::deliver(self, &orch_pane_id, envelope, &followup) {
                    log::warn!(
                        "[orchestration-pair] failed to notify orchestrator {orch_pane_id} of live Planner specialist: {e}"
                    );
                }
                // final count is the authoritative one for the nudge.
                let budget_notice = self.workspace_pane_budget_notice(&workspace_id);
                Ok(OrchestrationPairResult {
                    plan_id,
                    orchestrator_pane_id: orch_pane_id,
                    orchestrator_model,
                    planner_pane_id,
                    planner_model,
                    warning: orch_result.warning.or(planner_result.warning),
                    budget_notice,
                })
            }
            Err(e) => {
                let fix_command = degraded_fix_command(
                    &orch_pane_id,
                    &plan_id,
                    &params.planner_agent,
                    planner_model.as_deref(),
                );
                let followup = orchestrator_pair_followup_degraded(&fix_command, &e);
                let envelope = MessageEnvelope::reminder("CodeTerm", "orchestration-pair");
                if let Err(notify_err) =
                    Messenger::deliver(self, &orch_pane_id, envelope, &followup)
                {
                    log::warn!(
                        "[orchestration-pair] failed to notify orchestrator {orch_pane_id} of degraded pairing: {notify_err}"
                    );
                }
                Err(format!(
                    "Orchestrator spawned (pane {orch_pane_id}, plan {plan_id}) but Planner spawn failed: {e}. Complete the pair with: {fix_command}"
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

    /// Mirrors the plan-side sequence `create_orchestration_pair` performs
    /// around the two spawns: create (unowned) -> claim (orchestrator known)
    /// -> set_planner_ref (planner known). Both refs land on the same plan.
    #[test]
    fn happy_path_binds_both_orchestrator_and_planner_refs() {
        let (_root, store) = store_for_test();
        let plan = store
            .create_plan(
                "Orchestration".into(),
                _root.path().display().to_string(),
                OrchestratorRef::default(),
                Vec::new(),
                1,
            )
            .unwrap();

        store
            .claim(&plan.id, OrchestratorRef::from_pane("orch-1"), 2)
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

    /// Degraded path: the Orchestrator claims the plan, but the Planner spawn
    /// never happens (2nd spawn failure) — `planner` stays unset. This is the
    /// exact state the doctor pairing ladder's `planner_unpaired` check keys
    /// off, so the gap is recorded and actionable, never silently dropped.
    #[test]
    fn degraded_path_leaves_planner_ref_unbound_but_owner_recorded() {
        let (_root, store) = store_for_test();
        let plan = store
            .create_plan(
                "Orchestration".into(),
                _root.path().display().to_string(),
                OrchestratorRef::default(),
                Vec::new(),
                1,
            )
            .unwrap();

        store
            .claim(&plan.id, OrchestratorRef::from_pane("orch-1"), 2)
            .unwrap();

        let degraded = store.get(&plan.id).unwrap();
        assert_eq!(degraded.owner.pane_id.as_deref(), Some("orch-1"));
        assert!(degraded.planner.is_none());
    }

    #[test]
    fn orchestrator_task_names_real_plan_id_and_reuses_peer_constants() {
        let task = orchestrator_pair_task("pl_123");
        assert!(task.contains("pl_123"));
        assert!(task.contains(crate::prompts::PEER_PLANNER_CHARTER_LINE));
        assert!(task.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
        assert!(task.contains("attached Planner specialist"));
    }

    /// Regression for a live-dogfood finding: the pair-creation flow always
    /// spawns the Planner synchronously before the Orchestrator's task is
    /// even delivered, so telling it to spawn one "if not live" raced a real
    /// model into creating a redundant, mis-parented extra Worker pane.
    #[test]
    fn orchestrator_task_never_tells_model_to_spawn_its_own_planner() {
        let task = orchestrator_pair_task("pl_123");
        assert!(
            !task.contains("--role Planner --plan"),
            "task must never hand the model a copy-pasteable Planner spawn command: {task}"
        );
        assert!(task.contains("do NOT run"));
    }

    #[test]
    fn followup_ready_names_real_planner_pane_and_plan() {
        let msg = orchestrator_pair_followup_ready("pl_123", "pane-planner-1");
        assert!(msg.contains("pane-planner-1"));
        assert!(msg.contains("pl_123"));
        assert!(msg.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
        assert!(msg.contains("Planner specialist is live"));
    }

    #[test]
    fn degraded_fix_command_includes_model_when_present() {
        let cmd = degraded_fix_command("orch-1", "pl_123", "codex", Some("gpt-5"));
        assert!(cmd.contains("agent spawn codex"));
        assert!(cmd.contains("--model gpt-5"));
        assert!(cmd.contains("--parent orch-1"));
        assert!(cmd.contains("--role Planner --plan pl_123"));
    }

    #[test]
    fn degraded_fix_command_omits_model_flag_when_absent() {
        let cmd = degraded_fix_command("orch-1", "pl_123", "codex", None);
        assert!(!cmd.contains("--model"));
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
        let fix = degraded_fix_command("orch-1", "pl_123", "codex", None);
        let msg = orchestrator_pair_followup_degraded(&fix, "spawn timed out");
        assert!(msg.contains("spawn timed out"));
        assert!(msg.contains(&fix));
        assert!(msg.contains(crate::prompts::PEER_WAIVER_HINT));
    }
}
