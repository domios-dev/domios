use super::*;

impl ServiceContext {
    pub fn control_loop_run(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        now: u64,
    ) -> Result<ControlLoop, String> {
        self.authorize_plan_orchestrator(plan_id, caller)?;
        self.extensions
            .try_get::<super::CuratorTabs>()
            .ok_or("Curator runtime is unavailable")?;
        let read = self
            .plan_store
            .curator_schedule_projection(plan_id)
            .map_err(|error| error.to_string())?;
        let plan = read.metadata();
        let owner = plan.owner.tab_id.as_deref().ok_or("Orchestrator absent")?;
        let tab = self
            .mux
            .read()
            .get_tab(owner)
            .cloned()
            .ok_or("Orchestrator absent")?;
        self.available_mirror(scheduling::resolve_mirror(&tab)?)?;
        let eligible = read.eligible(
            self.configuration().orchestration.plan_reminders,
            self.curator_suspended_reason_for_fields(
                &plan.mode,
                &plan.status,
                &plan.team_id,
                read.complete(),
            )
            .is_some(),
        );
        drop(read);
        self.plan_store
            .edit_curator_state(plan_id, now, |plan| {
                curator::request_round(&mut plan.control_loop, eligible, now)
                    .map_err(crate::service::plan::PlanError::Validation)
            })
            .map_err(|error| error.to_string())?;
        self.reconcile_curator_id(plan_id, now)
            .map_err(|error| error.to_string())?;
        self.emit_committed_plan_changed(&plan_id);
        self.control_loop_show(plan_id)
    }
}
