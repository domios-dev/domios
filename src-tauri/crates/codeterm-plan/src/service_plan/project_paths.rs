use super::{PlanError, PlanStore};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanProjectUpdate {
    pub plan_id: String,
    pub project_path: String,
    pub canonical_project_path: String,
    pub changed: bool,
}

fn project_path_update(
    plan_id: &str,
    old_project: &str,
    old_canonical: &str,
    project: &str,
    canonical: &str,
) -> PlanProjectUpdate {
    PlanProjectUpdate {
        plan_id: plan_id.into(),
        project_path: project.into(),
        canonical_project_path: canonical.into(),
        changed: old_project != project || old_canonical != canonical,
    }
}

impl PlanStore {
    pub fn set_project_paths(
        &self,
        plan_id: &str,
        project: &str,
        canonical: &str,
        now_ms: u64,
    ) -> Result<PlanProjectUpdate, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let mut plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let update = project_path_update(
            &plan.id,
            &plan.project_path,
            &plan.canonical_project_path,
            project,
            canonical,
        );
        if update.changed {
            plan.project_path = update.project_path.clone();
            plan.canonical_project_path = update.canonical_project_path.clone();
            let now_ms = now_ms.max(plan.updated_at);
            self.save_in_writer(&mut plan, now_ms)
                .map_err(|error| PlanError::Io(error.to_string()))?;
        }
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_repair_decision_needs_no_old_folder_and_is_idempotent() {
        let update = project_path_update("plan", "D:/old", "D:/old", "C:/new", "C:/new");
        assert!(update.changed);
        assert_eq!(update.plan_id, "plan");
        assert_eq!(update.project_path, "C:/new");
        assert_eq!(update.canonical_project_path, "C:/new");
        assert!(!project_path_update("plan", "C:/new", "C:/new", "C:/new", "C:/new").changed);
        assert!(
            project_path_update("plan", "/linked", "/retired", "/linked", "/canonical").changed
        );
    }

    #[test]
    fn project_repair_persists_only_paths_and_keeps_all_plan_identity() {
        let temp = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(temp.path());
        let mut plan = crate::plan::Plan::new(
            "repair".into(),
            temp.path().display().to_string(),
            crate::plan::OrchestratorRef::from_tab("owner"),
        );
        plan.team_id = Some("existing-team".into());
        plan.tasks.push(serde_json::from_value(serde_json::json!({
            "id": "task-existing", "name": "task", "type": "implementation", "status": "pending",
            "execution_attempts": [{"id": "attempt-existing", "executor_tab_id": "worker-existing",
                "session_id": "session-existing"}]
        })).unwrap());
        store.save(&mut plan, 1).unwrap();
        let before = store.get(&plan.id).unwrap();
        let replacement = temp.path().join("replacement");
        std::fs::create_dir(&replacement).unwrap();
        let new = replacement.display().to_string();
        let outcome = store.set_project_paths(&plan.id, &new, &new, 2).unwrap();
        assert!(outcome.changed);
        let mut expected = before;
        expected.project_path = new.clone();
        expected.canonical_project_path = new.clone();
        expected.updated_at = 2;
        assert_eq!(store.get(&plan.id).unwrap(), expected);
        let first = store.get(&plan.id).unwrap();
        assert!(
            !store
                .set_project_paths(&plan.id, &new, &new, 3)
                .unwrap()
                .changed
        );
        assert_eq!(store.get(&plan.id).unwrap(), first);
        assert_eq!(
            PlanStore::new_for_test(temp.path()).get(&plan.id).unwrap(),
            first
        );
    }
}
