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
        let team_id = self.mint_id("team_test");
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
        self.create_plan_for_team_in_mode(title, project_path, owner, None, team_id, kinds, now_ms)
    }

    pub fn create_first_plan_for_team(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.create_plan_for_team_in_mode(title, project_path, owner, None, team_id, kinds, now_ms)
    }

    pub fn create_first_plan_for_team_with_planner(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        planner: PlannerRef,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.create_plan_for_team_in_mode(
            title,
            project_path,
            owner,
            Some(planner),
            team_id,
            kinds,
            now_ms,
        )
    }

    fn create_plan_for_team_in_mode(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        planner: Option<PlannerRef>,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
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
        if self.id_taken(&plan.id) {
            plan.id = self.mint_id("pl");
        }
        plan.team_id = Some(team_id);
        plan.planner = planner;
        plan.milestone_kinds = kinds;
        if let Err(error) = self.save_in_writer(&mut plan, now_ms) {
            self.plans.write().remove(&plan.id);
            self.index.write().remove(&plan.id);
            return Err(PlanError::Io(error.to_string()));
        }
        Ok(plan)
    }

    pub fn active_plan_for_team(&self, team_id: &str) -> Option<(String, String)> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return None;
        }
        self.plans.read().values().find_map(|arc| {
            let plan = arc.lock();
            (plan.status == PlanStatus::Active && plan.team_id.as_deref() == Some(team_id))
                .then(|| (plan.id.clone(), plan.title.clone()))
        })
    }

    #[cfg(test)]
    pub fn bind_plan_to_team(
        &self,
        plan_id: &str,
        team_id: &str,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            });
        }
        let _writer_turn = self.mutation_writer.enter();
        if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team_id) {
            if existing_plan_id != plan_id {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team_id.to_string(),
                    existing_plan_id,
                    existing_title,
                });
            }
        }
        self.with_plan_in_writer(plan_id, now_ms, |plan| {
            if let Some(bound_team_id) = plan.team_id.as_deref() {
                if bound_team_id != team_id {
                    return Err(PlanError::PlanAlreadyBoundToTeam {
                        plan_id: plan.id.clone(),
                        team_id: bound_team_id.to_string(),
                    });
                }
            }
            plan.team_id = Some(team_id.to_string());
            Ok(plan.clone())
        })
    }

    pub fn adopt_plan_for_team(
        &self,
        plan_id: &str,
        team_id: &str,
        owner: OrchestratorRef,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            });
        }
        let _writer_turn = self.mutation_writer.enter();
        if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team_id) {
            if existing_plan_id != plan_id {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team_id.to_string(),
                    existing_plan_id,
                    existing_title,
                });
            }
        }
        self.with_plan_in_writer(plan_id, now_ms, |plan| {
            if let Some(bound_team_id) = plan.team_id.as_deref() {
                if bound_team_id != team_id {
                    return Err(PlanError::PlanAlreadyBoundToTeam {
                        plan_id: plan.id.clone(),
                        team_id: bound_team_id.to_string(),
                    });
                }
            }
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
            plan.team_id = Some(team_id.to_string());
            plan.status = PlanStatus::Active;
            Ok(plan.clone())
        })
    }

    pub fn transfer_plan_to_team(
        &self,
        plan_id: &str,
        team_id: &str,
        to: OrchestratorRef,
        reason: &str,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = team_id.trim();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: plan_id.into(),
            });
        }
        let _writer_turn = self.mutation_writer.enter();
        if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team_id) {
            if existing_plan_id != plan_id {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team_id.to_string(),
                    existing_plan_id,
                    existing_title,
                });
            }
        }
        self.with_plan_in_writer(plan_id, now_ms, |plan| {
            plan.owner_history.push(TransferRecord {
                from: plan.owner.clone(),
                to: to.clone(),
                ts: now_ms,
                reason: reason.into(),
            });
            plan.owner = to;
            plan.planner = None;
            plan.team_id = Some(team_id.to_string());
            Ok(plan.clone())
        })
    }
}
