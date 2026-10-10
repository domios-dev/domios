use super::*;
use crate::service::plan::mutations::agent_lifecycle::VerifiedWorkerTransferBinding;
use crate::service::AuthenticatedPlanCaller;
use codeterm_types::team::TeamLifecycle;
use serde::Serialize;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) struct WorkerTransferOutcome {
    pub status: &'static str,
    pub receipt_id: String,
    pub worker_tab_id: String,
    pub source_manager_tab_id: String,
    pub destination_manager_tab_id: String,
    pub source_team_id: String,
    pub destination_team_id: String,
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
}

impl ServiceContext {
    pub(crate) fn task_bound_worker_transfer_candidate(
        &self,
        worker_tab_id: &str,
        destination_manager_tab_id: &str,
    ) -> bool {
        let mux = self.mux.read();
        let Some(worker) = mux.get_tab(worker_tab_id) else {
            return false;
        };
        if worker.role != Some(AgentRole::Worker)
            || worker.parent_tab_id.as_deref() == Some(destination_manager_tab_id)
        {
            return false;
        }
        self.plan_store.list_full().iter().any(|plan| {
            plan.tasks.iter().any(|task| {
                (!task.status.is_terminal()
                    && (task.assigned_tab_id.as_deref() == Some(worker_tab_id)
                        || task.lane.tab_id.as_deref() == Some(worker_tab_id)))
                    || task.execution_attempts.iter().any(|attempt| {
                        attempt.executor_tab_id == worker_tab_id && attempt.closed_at.is_none()
                    })
            })
        })
    }

    pub(crate) fn transfer_task_bound_worker(
        &self,
        caller: &AuthenticatedPlanCaller,
        worker_tab_id: &str,
        destination_manager_tab_id: &str,
    ) -> Result<WorkerTransferOutcome, String> {
        let mut mux = self.mux.write();
        let worker = mux
            .get_tab(worker_tab_id)
            .ok_or_else(|| "worker transfer refused: Worker tab not found".to_string())?
            .clone();
        if worker.role != Some(AgentRole::Worker)
            || worker.parent_tab_id.as_deref() == Some(destination_manager_tab_id)
        {
            return Err("worker transfer refused: target is not a cross-parent Worker".into());
        }
        if !mux.get_children_ids(worker_tab_id).is_empty() {
            return Err("worker transfer refused: Worker has child tabs".into());
        }
        if worker.report_target_mesh_peer.is_some()
            || worker.pending_delivery.is_some()
            || worker.ownership.is_some()
        {
            return Err("worker transfer refused: Worker has remote or pending state".into());
        }
        let worker_workspace = mux
            .find_workspace_for_tab(worker_tab_id)
            .ok_or_else(|| "worker transfer refused: Worker workspace is missing".to_string())?;
        let (session_id, nonce) = verified_session(&worker)?;
        if !crate::service::plan_reconcile::tab_has_live_agent(&worker) {
            return Err("worker transfer refused: Worker session is not live".into());
        }
        let source_manager_tab_id = worker
            .parent_tab_id
            .as_deref()
            .ok_or_else(|| "worker transfer refused: source manager is missing".to_string())?
            .to_string();
        let source = verified_orchestrator(&mux, &source_manager_tab_id, &worker_workspace)?;
        let (source_session_id, source_nonce) = verified_session(source)?;
        if caller.tab_id.as_deref() != Some(source_manager_tab_id.as_str())
            || caller.session_id.as_deref() != Some(source_session_id)
            || caller.binding_nonce.as_deref() != Some(source_nonce)
        {
            return Err(
                "worker transfer refused: caller is not the authenticated source manager".into(),
            );
        }
        let destination =
            verified_orchestrator(&mux, destination_manager_tab_id, &worker_workspace)?;
        let (destination_session_id, _) = verified_session(destination)?;
        let source_team_id = source
            .team_id
            .clone()
            .ok_or_else(|| "worker transfer refused: source manager has no Team".to_string())?;
        let destination_team_id = destination.team_id.clone().ok_or_else(|| {
            "worker transfer refused: destination manager has no Team".to_string()
        })?;
        let source_team = mux
            .get_team(&source_team_id)
            .ok_or_else(|| "worker transfer refused: source Team is missing".to_string())?;
        let destination_team = mux
            .get_team(&destination_team_id)
            .ok_or_else(|| "worker transfer refused: destination Team is missing".to_string())?;
        if source_team.lifecycle != TeamLifecycle::Active
            || destination_team.lifecycle != TeamLifecycle::Active
            || worker.team_id.as_deref() != Some(source_team_id.as_str())
            || !unique_team_anchor(&mux, &source_team_id, &source_manager_tab_id)
            || !unique_team_anchor(&mux, &destination_team_id, destination_manager_tab_id)
        {
            return Err("worker transfer refused: manager Team anchor is not active".into());
        }
        if source_manager_tab_id == destination_manager_tab_id {
            return Err("worker transfer refused: source and destination managers match".into());
        }

        let plan_binding = worker
            .task_binding
            .as_ref()
            .filter(|binding| binding.spawn_handle.is_none())
            .ok_or_else(|| {
                "worker transfer refused: Worker has no exact plan task binding".to_string()
            })?;
        let plan_id = plan_binding
            .plan_id
            .as_deref()
            .ok_or_else(|| "worker transfer refused: task binding has no plan".to_string())?;
        let task_id = plan_binding
            .task_id
            .as_deref()
            .ok_or_else(|| "worker transfer refused: task binding has no task".to_string())?;
        let attempt_id = plan_binding
            .attempt_id
            .as_deref()
            .ok_or_else(|| "worker transfer refused: task binding has no attempt".to_string())?;

        let plan = self
            .plan_store
            .get(plan_id)
            .ok_or_else(|| "worker transfer refused: bound plan is missing".to_string())?;
        if plan.owner.tab_id.as_deref() != Some(destination_manager_tab_id)
            || plan
                .owner
                .session_id
                .as_deref()
                .is_some_and(|session_id| session_id != destination_session_id)
            || plan.team_id.as_deref() != Some(destination_team_id.as_str())
        {
            return Err("worker transfer refused: destination does not own the bound plan".into());
        }
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| "worker transfer refused: bound task is missing".to_string())?;
        let attempt = task
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == attempt_id)
            .ok_or_else(|| "worker transfer refused: bound attempt is missing".to_string())?;
        let provider = worker
            .agent_type
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "worker transfer refused: live provider is missing".to_string())?;
        let all_plans = self.plan_store.list_full();
        let task_bindings = all_plans
            .iter()
            .flat_map(|candidate| {
                candidate.tasks.iter().filter_map(|task| {
                    (!task.status.is_terminal()
                        && task.assigned_tab_id.as_deref() == Some(worker_tab_id))
                    .then_some((candidate.id.as_str(), task.id.as_str()))
                })
            })
            .collect::<Vec<_>>();
        if task_bindings.len() != 1
            || task_bindings[0] != (plan_id, task_id)
            || task.lane.tab_id.as_deref() != Some(worker_tab_id)
            || task.lane.workspace_id.as_deref() != Some(worker_workspace.as_str())
            || task.lane.worktree_path.as_deref().is_none_or(str::is_empty)
            || task.lane.branch.as_deref().is_none_or(str::is_empty)
            || worker.cwd.as_deref() != task.lane.worktree_path.as_deref()
            || attempt.executor_tab_id != worker_tab_id
            || attempt.session_id.as_deref() != Some(session_id)
            || attempt.session_binding_nonce.as_deref() != Some(nonce)
            || attempt.provider.as_deref() != Some(provider)
            || attempt.placement != codeterm_types::plan::WorkerPlacement::IsolatedLane
            || attempt.acknowledged_at.is_none()
            || attempt.closed_at.is_some()
            || attempt.retired_at.is_some()
            || attempt.mesh_peer.is_some()
            || attempt.remote_dispatch_state.is_some()
            || attempt.worktree_path.as_deref() != task.lane.worktree_path.as_deref()
            || task.interruption.handoff_required
            || task.interruption.checkpoint.is_some()
            || task.interruption.owner_tab_id.as_deref() != Some(worker_tab_id)
            || task.interruption.owner_session_id.as_deref() != Some(session_id)
            || attempt.interruption.owner_epoch != task.interruption.owner_epoch
        {
            return Err("worker transfer refused: task ownership is ambiguous or stale".into());
        }

        let verified = VerifiedWorkerTransferBinding {
            plan_id: plan_id.to_string(),
            team_id: destination_team_id.clone(),
            owner_tab_id: destination_manager_tab_id.to_string(),
            owner_session_id: destination_session_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt_id.to_string(),
            worker_tab_id: worker_tab_id.to_string(),
            workspace_id: worker_workspace,
            worktree_path: task.lane.worktree_path.clone().ok_or_else(|| {
                "worker transfer refused: isolated lane path is missing".to_string()
            })?,
            session_id: session_id.to_string(),
            binding_nonce: nonce.to_string(),
            provider: provider.to_string(),
            owner_epoch: task.interruption.owner_epoch,
        };
        let mut staged = mux.begin_team_close_transaction();
        let staged_state = staged.state_mut();
        let destination_depth = staged_state
            .get_tab(destination_manager_tab_id)
            .map(|tab| tab.depth)
            .ok_or_else(|| {
                "worker transfer refused: destination manager disappeared".to_string()
            })?;
        let staged_worker = staged_state
            .get_tab_mut(worker_tab_id)
            .ok_or_else(|| "worker transfer refused: Worker disappeared".to_string())?;
        staged_worker.parent_tab_id = Some(destination_manager_tab_id.to_string());
        staged_worker.team_id = Some(destination_team_id.clone());
        staged_worker.depth = destination_depth.saturating_add(1);
        staged_worker.can_spawn = false;
        staged_worker.report_target_tab_id = Some(destination_manager_tab_id.to_string());

        self.plan_store
            .with_verified_worker_transfer_binding(&verified, || staged.commit(&mut mux, &[]))
            .map_err(|error| error.to_string())?;
        drop(mux);

        let outcome = WorkerTransferOutcome {
            status: "committed_observation_unconfirmed",
            receipt_id: uuid::Uuid::new_v4().to_string(),
            worker_tab_id: worker_tab_id.to_string(),
            source_manager_tab_id,
            destination_manager_tab_id: destination_manager_tab_id.to_string(),
            source_team_id,
            destination_team_id,
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt_id.to_string(),
        };
        log::info!(
            "[worker-transfer] committed_observation_unconfirmed receipt={} worker={} source_manager={} destination_manager={} source_team={} destination_team={} plan={} task={} attempt={}",
            outcome.receipt_id,
            outcome.worker_tab_id,
            outcome.source_manager_tab_id,
            outcome.destination_manager_tab_id,
            outcome.source_team_id,
            outcome.destination_team_id,
            outcome.plan_id,
            outcome.task_id,
            outcome.attempt_id
        );
        self.record_role_transition(worker_tab_id);
        self.schedule_session_persist_debounced();
        self.emit_state_changed();
        Ok(outcome)
    }
}

fn verified_orchestrator<'a>(
    mux: &'a MultiplexerState,
    tab_id: &str,
    workspace_id: &str,
) -> Result<&'a Tab, String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| "worker transfer refused: manager tab is missing".to_string())?;
    if tab.role != Some(AgentRole::Manager)
        || tab.role_profile.as_deref()
            != Some(codeterm_types::agent::role_kind::builtins::ORCHESTRATOR)
        || !tab.anchors_team()
        || !crate::service::plan_reconcile::tab_has_live_agent(tab)
        || verified_session(tab).is_err()
        || mux.find_workspace_for_tab(tab_id).as_deref() != Some(workspace_id)
        || tab.report_target_mesh_peer.is_some()
    {
        return Err("worker transfer refused: manager is not a live local Orchestrator".into());
    }
    Ok(tab)
}

fn unique_team_anchor(mux: &MultiplexerState, team_id: &str, manager_tab_id: &str) -> bool {
    let anchors = mux
        .team_member_tab_ids(team_id)
        .into_iter()
        .filter(|tab_id| mux.get_tab(tab_id).is_some_and(Tab::anchors_team))
        .collect::<Vec<_>>();
    anchors.len() == 1 && anchors[0] == manager_tab_id
}
