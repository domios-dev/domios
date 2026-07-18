use super::*;
use crate::service_plan::paths;

impl PlanStore {
    pub fn create_plan(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.create_plan_for_team(title, project_path, owner, None, kinds, now_ms)
    }

    pub fn create_plan_for_team(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: Option<String>,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        self.create_plan_for_team_in_mode(title, project_path, owner, team_id, kinds, now_ms, false)
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
        self.create_plan_for_team_in_mode(
            title,
            project_path,
            owner,
            Some(team_id),
            kinds,
            now_ms,
            true,
        )
    }

    fn create_plan_for_team_in_mode(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: Option<String>,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
        adopt_matching_legacy: bool,
    ) -> Result<Plan, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let team_id = team_id
            .map(|team| team.trim().to_string())
            .filter(|team| !team.is_empty());
        if let Some(team) = team_id.as_deref() {
            if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team) {
                return Err(PlanError::TeamAlreadyHasActivePlan {
                    team_id: team.to_string(),
                    existing_plan_id,
                    existing_title,
                });
            }
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
        if adopt_matching_legacy {
            let matching_ids: Vec<String> = self
                .plans
                .read()
                .values()
                .filter_map(|arc| {
                    let plan = arc.lock();
                    let stored_project = if plan.canonical_project_path.trim().is_empty() {
                        &plan.project_path
                    } else {
                        &plan.canonical_project_path
                    };
                    (plan.status == PlanStatus::Active
                        && plan.team_id.is_none()
                        && plan.title.trim() == title.trim()
                        && std::path::Path::new(stored_project) == project_path)
                        .then(|| plan.id.clone())
                })
                .collect();
            if let [plan_id] = matching_ids.as_slice() {
                let team_id = team_id.as_deref().expect("first plan requires a team");
                return self.with_plan_in_writer(plan_id, now_ms, |plan| {
                    plan.team_id = Some(team_id.to_string());
                    Ok(plan.clone())
                });
            }
        }
        let project_path = project_path.display().to_string();
        let mut plan = Plan::new(title, project_path, owner);
        if self.id_taken(&plan.id) {
            plan.id = self.mint_id("pl");
        }
        plan.team_id = team_id;
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

    pub fn bind_plan_to_team(
        &self,
        plan_id: &str,
        team_id: &str,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        let team_id = team_id.trim();
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
}
