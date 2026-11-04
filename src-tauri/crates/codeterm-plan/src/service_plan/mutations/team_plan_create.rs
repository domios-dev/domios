use super::*;
use crate::service_plan::paths;

impl PlanStore {
    #[cfg(test)]
    pub fn create_plan(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = self.mint_id("team_test")?;
        self.create_plan_for_team(title, project_path, owner, team_id, kinds, now_ms)
    }

    pub fn create_plan_for_team(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.create_plan_for_team_in_mode(title, project_path, owner, team_id, kinds, now_ms, true)
    }

    fn create_plan_for_team_in_mode(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
        observed: bool,
    ) -> Result<Plan, PlanError> {
        crate::service_plan::text_bounds::validate_plan_label("plan title", &title)?;
        let writer_turn = self.mutation_writer.enter();
        let team_id = team_id.trim().to_string();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: "new plan".into(),
            });
        }
        if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(&team_id) {
            return Err(PlanError::TeamAlreadyHasActivePlan {
                team_id,
                existing_plan_id,
                existing_title,
            });
        }
        // Require absolute project paths because relative canonicalization uses the daemon cwd and can bind the wrong checkout.
        if !std::path::Path::new(&project_path).is_absolute() {
            return Err(PlanError::Io(format!(
                "project path must be an absolute checkout path, got '{project_path}'; \
pass an absolute path with --project, or omit --project to use the current working directory"
            )));
        }
        // Store every effort against the main project root, never a worktree subdirectory.
        let project_path = paths::project_root(std::path::Path::new(&project_path));
        let project_metadata = std::fs::metadata(&project_path).map_err(|error| {
            PlanError::Io(format!(
                "read project path {}: {error}",
                project_path.display()
            ))
        })?;
        if !project_metadata.is_dir() {
            return Err(PlanError::Io(format!(
                "project path is not a directory: {}",
                project_path.display()
            )));
        }
        let project_path = project_path.display().to_string();
        let mut plan = Plan::new(title, project_path, owner);
        if self.id_taken(&plan.id)? {
            plan.id = self.mint_id("pl")?;
        }
        plan.team_id = Some(team_id);
        plan.milestone_kinds = kinds;
        if let Err(error) = self.import_payload_in_writer(&writer_turn, &mut plan, now_ms) {
            self.forget_plan(&plan.id);
            return Err(PlanError::Io(error.to_string()));
        }
        if observed {
            self.publish_product_created();
        }
        Ok(plan)
    }

    pub fn active_plan_for_team(&self, team_id: &str) -> Option<(String, String)> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return None;
        }
        self.index.read().values().find_map(|row| {
            (row.status == PlanStatus::Active && row.team_id.as_deref() == Some(team_id))
                .then(|| (row.id.clone(), row.title.clone()))
        })
    }

    #[cfg(test)]
    pub fn bind_plan_to_team(
        &self,
        plan_id: &str,
        team_id: &str,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.bind_team_rows_for_test(plan_id, team_id, None, now_ms)
    }

    #[cfg(test)]
    pub fn adopt_plan_for_team(
        &self,
        plan_id: &str,
        team_id: &str,
        owner: OrchestratorRef,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.bind_team_rows_for_test(plan_id, team_id, Some(owner), now_ms)
    }

    #[cfg(test)]
    fn bind_team_rows_for_test(
        &self,
        plan_id: &str,
        team_id: &str,
        owner: Option<OrchestratorRef>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            });
        }
        let turn = self.mutation_writer.enter();
        if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team_id) {
            if existing_plan_id != plan_id {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team_id.into(),
                    existing_plan_id,
                    existing_title,
                });
            }
        }
        let selector = PlanMatch::Ids(vec![plan_id.into()]);
        let plans = self
            .database
            .observe_team_rows(&selector, None, &[], false)?;
        let mut staged = plans.clone();
        let plan = staged.first_mut().ok_or(PlanError::NotFound)?;
        if let Some(bound) = plan.team_id.as_deref() {
            if bound != team_id {
                return Err(PlanError::PlanAlreadyBoundToTeam {
                    plan_id: plan.id.clone(),
                    team_id: bound.into(),
                });
            }
        }
        if let Some(owner) = owner {
            if plan.owner != owner {
                plan.owner_history.push(TransferRecord {
                    from: plan.owner.clone(),
                    to: owner.clone(),
                    ts: now_ms,
                    reason: format!("adopted by Team {team_id}"),
                });
            }
            plan.owner = owner;
            plan.planner = None;
            plan.status = PlanStatus::Active;
            plan.orphaned = None;
        }
        plan.team_id = Some(team_id.into());
        self.commit_team_row_patches(&turn, selector, plans, staged, now_ms)?;
        self.try_get(plan_id)
    }
}

#[cfg(test)]
mod tests;

impl PlanStore {
    pub fn bind_product_observer(&self, observer: std::sync::Arc<dyn Fn() + Send + Sync>) {
        *self.product_observer.write() = Some(observer);
    }
    pub(in crate::service_plan) fn publish_product_created(&self) {
        let observer = self.product_observer.read().clone();
        if let Some(observer) = observer {
            observer();
        }
    }
}
