//! Paired creation (task_7e5f9354): spawn an Orchestrator + Planner together,
//! linked from birth via a freshly-created plan. Builds entirely on the
//! peer-pairing primitives already wired by the peer-foundation
//! (`crate::plan::peer`) and Lane 1 (`crate::service::plan::peer_mandate`,
//! `role_profile == "planner"` binding in `Self::spawn_agent`) — this module
//! only sequences the two spawns + the plan create through one call.

use serde::{Deserialize, Serialize};

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
}

fn epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Orchestrator's first task. The Planner pane doesn't exist yet at this
/// point (it spawns second), so the peer command names the real plan id but
/// keeps a placeholder for the not-yet-real Planner pane id.
fn orchestrator_pair_task(plan_id: &str) -> String {
    let spawn_cmd =
        format!("codeterm agent spawn <planner-provider> --role Planner --plan {plan_id}");
    let mandate = crate::prompts::peer_pairing_mandate_full(&spawn_cmd);
    format!(
        "You are the Orchestrator for plan {plan_id}. A Planner peer is being spawned for this plan now.\n\n{mandate}"
    )
}

/// Follow-up delivered to the Orchestrator once the Planner spawn succeeds —
/// the first point both real pane ids are known.
fn orchestrator_pair_followup_ready(plan_id: &str, planner_pane_id: &str) -> String {
    format!(
        "Your Planner peer is live: pane {planner_pane_id} for plan {plan_id}. {}",
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

fn orchestrator_pair_followup_degraded(fix_command: &str, error: &str) -> String {
    format!(
        "Planner spawn failed ({error}). This plan has no live Planner peer yet.\n\n{}\n\nComplete pairing with:\n  {fix_command}",
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

        let now_ms = epoch_ms();
        let (workspace_id, project_path) = {
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
            (ws_id, cwd)
        };

        let title = params
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "Orchestration".to_string());
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

        let planner_model = params
            .planner_model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| crate::agent::model_catalog::resolve_default(&params.planner_agent));

        let planner_params = SpawnAgentParams {
            agent: Some(params.planner_agent.clone()),
            model: planner_model.clone(),
            role: Some("Worker".into()),
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
                let followup = orchestrator_pair_followup_ready(&plan_id, &planner_pane_id);
                let envelope = MessageEnvelope::reminder("CodeTerm", "orchestration-pair");
                if let Err(e) = Messenger::deliver(self, &orch_pane_id, envelope, &followup) {
                    log::warn!(
                        "[orchestration-pair] failed to notify orchestrator {orch_pane_id} of live planner peer: {e}"
                    );
                }
                Ok(OrchestrationPairResult {
                    plan_id,
                    orchestrator_pane_id: orch_pane_id,
                    orchestrator_model,
                    planner_pane_id,
                    planner_model,
                    warning: orch_result.warning.or(planner_result.warning),
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
        assert!(task.contains("This plan has a Planner peer"));
    }

    #[test]
    fn followup_ready_names_real_planner_pane_and_plan() {
        let msg = orchestrator_pair_followup_ready("pl_123", "pane-planner-1");
        assert!(msg.contains("pane-planner-1"));
        assert!(msg.contains("pl_123"));
        assert!(msg.contains(crate::prompts::PEER_ORCH_DEFERENCE_LINE));
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
    fn degraded_followup_names_exact_fix_command_and_waiver_hint() {
        let fix = degraded_fix_command("orch-1", "pl_123", "codex", None);
        let msg = orchestrator_pair_followup_degraded(&fix, "spawn timed out");
        assert!(msg.contains("spawn timed out"));
        assert!(msg.contains(&fix));
        assert!(msg.contains(crate::prompts::PEER_WAIVER_HINT));
    }
}
