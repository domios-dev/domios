use super::*;

#[derive(Debug, Clone)]
pub struct AgentLifecyclePlanChange {
    pub tab_id: String,
    pub preserve_owned_plans: bool,
    pub preserve_task_bindings_for_owner: Option<String>,
    pub planner_binding: Option<(String, Option<PlannerRef>)>,
    pub plan_rebind: Option<AgentLifecyclePlanRebind>,
}

#[derive(Debug, Clone)]
pub struct AgentLifecyclePlanRebind {
    pub plan_id: String,
    pub owner: OrchestratorRef,
    pub team_id: String,
    pub reason: String,
    pub activate: bool,
    pub clear_planner: bool,
}

#[derive(Debug)]
pub struct AgentLifecyclePlanTransaction {
    original_plans: Vec<Plan>,
    changed_plan_ids: Vec<String>,
}

impl AgentLifecyclePlanTransaction {
    pub fn changed_plan_ids(&self) -> &[String] {
        &self.changed_plan_ids
    }
}

impl PlanStore {
    pub fn apply_agent_lifecycle_change(
        &self,
        change: AgentLifecyclePlanChange,
        now_ms: u64,
    ) -> Result<AgentLifecyclePlanTransaction, PlanError> {
        self.apply_agent_lifecycle_changes(vec![change], now_ms)
    }

    pub fn apply_agent_lifecycle_changes(
        &self,
        changes: Vec<AgentLifecyclePlanChange>,
        now_ms: u64,
    ) -> Result<AgentLifecyclePlanTransaction, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let mut originals = Vec::new();
        let mut staged = Vec::new();
        let plans = self.list_full();

        for original in plans {
            let mut candidate = original.clone();
            for change in &changes {
                apply_change(&mut candidate, change, now_ms);
            }
            if candidate != original {
                candidate.updated_at = now_ms;
                tag_model::normalize_plan(&mut candidate);
                originals.push(original);
                staged.push(candidate);
            }
        }

        let changed_plan_ids = staged.iter().map(|plan| plan.id.clone()).collect();
        let mut persisted = 0;
        let mut paths = Vec::new();
        for candidate in &staged {
            match self.persist_plan_and_spec(candidate) {
                Ok(path) => paths.push(path),
                Err(error) => {
                    for original in originals.iter().take(persisted) {
                        let _ = self.persist_plan_and_spec(original);
                    }
                    return Err(PlanError::Io(error.to_string()));
                }
            }
            persisted += 1;
        }
        for (candidate, path) in staged.iter().zip(paths) {
            self.commit_agent_lifecycle_plan(candidate, path)?;
        }

        Ok(AgentLifecyclePlanTransaction {
            original_plans: originals,
            changed_plan_ids,
        })
    }

    pub fn rollback_agent_lifecycle_change(
        &self,
        transaction: AgentLifecyclePlanTransaction,
    ) -> Result<(), PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let mut restored = Vec::new();
        for original in &transaction.original_plans {
            let path = self
                .persist_plan_and_spec(original)
                .map_err(|error| PlanError::Io(error.to_string()))?;
            restored.push(path);
        }
        for (original, path) in transaction.original_plans.iter().zip(restored) {
            self.commit_agent_lifecycle_plan(original, path)?;
        }
        Ok(())
    }

    fn commit_agent_lifecycle_plan(
        &self,
        staged: &Plan,
        path: std::path::PathBuf,
    ) -> Result<(), PlanError> {
        let arc = self
            .plans
            .read()
            .get(&staged.id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        {
            let mut plan = arc.lock();
            *plan = staged.clone();
        }
        self.note_plan_write(staged, &path);
        self.record_committed_snapshot(staged);
        self.index
            .write()
            .insert(staged.id.clone(), Self::index_entry(staged, path));
        self.reset_bound_tabs(staged);
        Ok(())
    }
}

fn apply_change(plan: &mut Plan, change: &AgentLifecyclePlanChange, now_ms: u64) {
    let tab_id = change.tab_id.as_str();
    let rebound_here = change
        .plan_rebind
        .as_ref()
        .is_some_and(|rebind| rebind.plan_id == plan.id);
    if !change.preserve_owned_plans && !rebound_here && plan.owner.tab_id.as_deref() == Some(tab_id)
    {
        plan.owner = OrchestratorRef::default();
        plan.team_id = None;
        plan.planner = None;
        plan.status = PlanStatus::Archived;
    }

    let planner_is_tab = matches!(
        &plan.planner,
        Some(PlannerRef::Tab { tab_id: bound, .. }) if bound == tab_id
    );
    let replacement_is_unchanged = change
        .planner_binding
        .as_ref()
        .filter(|(plan_id, _)| plan_id == &plan.id)
        .is_some_and(|(_, planner)| plan.planner == *planner);
    if planner_is_tab && !replacement_is_unchanged {
        plan.planner = None;
        clear_planner_delivery_provenance(plan);
    }

    let task_binding_remains_valid = change
        .preserve_task_bindings_for_owner
        .as_deref()
        .is_some_and(|owner| plan.owner.tab_id.as_deref() == Some(owner));
    if !task_binding_remains_valid {
        for task in &mut plan.tasks {
            let assigned_here = task.assigned_tab_id.as_deref() == Some(tab_id)
                || task.lane.tab_id.as_deref() == Some(tab_id);
            if !assigned_here {
                continue;
            }
            let assigned = task.assigned_tab_id.clone();
            task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            clear_task_assignment(task);
            if task.status == TaskStatus::InProgress {
                task.status = TaskStatus::Pending;
                task.status_changed_at = now_ms;
            }
            task.updated_at = now_ms;
        }
    }

    if let Some(rebind) = &change.plan_rebind {
        if plan.id == rebind.plan_id {
            let ownership_changed =
                plan.owner != rebind.owner || plan.team_id.as_deref() != Some(&rebind.team_id);
            if ownership_changed {
                plan.owner_history.push(TransferRecord {
                    from: plan.owner.clone(),
                    to: rebind.owner.clone(),
                    ts: now_ms,
                    reason: rebind.reason.clone(),
                });
                plan.owner = rebind.owner.clone();
                plan.team_id = Some(rebind.team_id.clone());
            }
            if rebind.clear_planner && plan.planner.is_some() {
                plan.planner = None;
                clear_planner_delivery_provenance(plan);
            }
            if rebind.activate {
                plan.status = PlanStatus::Active;
            }
        }
    }
    if let Some((plan_id, planner)) = &change.planner_binding {
        if plan.id == *plan_id {
            let changed = plan.planner != *planner;
            plan.planner = planner.clone();
            if changed {
                clear_planner_delivery_provenance(plan);
            }
        }
    }
}

fn clear_planner_delivery_provenance(plan: &mut Plan) {
    for entry in &mut plan.planner_inbox {
        entry.phase = crate::plan::PlannerDeliveryPhase::PreSend;
        entry.tab_id = None;
        entry.session_id = None;
        entry.binding_nonce = None;
        entry.transport = None;
        entry.last_error = None;
    }
    for request in &mut plan.planner_requests {
        if matches!(
            request.state,
            crate::plan::PlannerRequestState::Accepted
                | crate::plan::PlannerRequestState::InProgress
        ) {
            request.state = crate::plan::PlannerRequestState::Queued;
            request.accepted_at = None;
            request.lease_expires_at = None;
        }
        request.delivered_to_tab_id = None;
        request.delivered_to_session_id = None;
        request.delivered_with_binding_nonce = None;
    }
}
