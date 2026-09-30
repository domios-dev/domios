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
        self.create_plan_for_team_in_mode(title, project_path, owner, team_id, kinds, now_ms)
    }

    fn create_plan_for_team_in_mode(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        team_id: String,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        crate::service_plan::text_bounds::validate_plan_label("plan title", &title)?;
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
        if self.id_taken(&plan.id) {
            plan.id = self.mint_id("pl");
        }
        plan.team_id = Some(team_id);
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
            plan.orphaned = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// codeterm#384: a bare/relative `--project` name must never be resolved
    /// against the daemon's own process cwd — canonicalizing a relative path
    /// resolves it against whatever directory the daemon process happens to
    /// be running from (e.g. the CodeTerm data dir), not the caller's actual
    /// checkout. The refusal below is filesystem-independent by construction
    /// (checked before any path is touched), so it holds even for the exact
    /// failing case: a bare name that ALSO exists as a directory under the
    /// CodeTerm data dir — proven here by a bare name that indeed exists as a
    /// directory relative to the current process cwd, which must still not
    /// be enough to accept it.
    #[test]
    fn bare_project_name_is_refused_even_when_it_exists_under_the_process_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let bare_name = "src"; // a real directory relative to this crate's own test cwd
        assert!(
            cwd.join(bare_name).is_dir(),
            "fixture precondition: {bare_name} must exist as a directory relative to cwd"
        );
        let store_dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(store_dir.path());
        let result = store.create_plan(
            "Mis-rooted".into(),
            bare_name.into(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        );
        assert!(
            matches!(result, Err(PlanError::Io(_))),
            "a bare project name must be refused at creation, not recorded: {result:?}"
        );
    }

    #[test]
    fn absolute_project_path_records_the_resolved_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let plan = store
            .create_plan(
                "Rooted".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_tab("orch"),
                vec![],
                1,
            )
            .expect("an absolute, existing checkout path must be accepted");
        assert_eq!(
            plan.project_path,
            paths::project_root(dir.path()).display().to_string()
        );
    }
}
