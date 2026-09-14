use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

impl PlanStore {
    pub fn rm_task(&self, plan_id: &str, task_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let canonical_id = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .map(|task| task.id.clone())
            .ok_or(PlanError::TaskNotFound)?;
        if plan
            .tasks
            .iter()
            .find(|task| task.id == canonical_id)
            .is_some_and(|task| task.status == crate::plan::TaskStatus::InReview)
        {
            return Err(PlanError::TransitionDenied {
                from: crate::plan::TaskStatus::InReview,
                to: crate::plan::TaskStatus::Cancelled,
            });
        }
        let dependent_task_ids: Vec<String> = plan
            .tasks
            .iter()
            .filter(|task| {
                task.id != canonical_id && task.depends_on.iter().any(|id| id == &canonical_id)
            })
            .map(|task| task.id.clone())
            .collect();
        if !dependent_task_ids.is_empty() {
            let result = Err(PlanError::DependencyTaskReferenced {
                task_id: canonical_id.clone(),
                dependent_task_ids: dependent_task_ids.clone(),
            });
            return pending_after_verified(
                result,
                self.verify_task_removal_rejected(plan_id, &canonical_id, &dependent_task_ids),
                Some(canonical_id),
            );
        }
        let write = self.with_plan(plan_id, now_ms, |p| {
            let pos = crate::plan::task_key::find_task_index(&p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            p.tasks.remove(pos);
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_removed(plan_id, task_id),
            Some(task_id.to_string()),
        )?;
        self.remove_task_attachment_dir(&plan, &canonical_id)
    }

    pub fn rm_all_tasks(&self, plan_id: &str, now_ms: u64) -> Result<usize, PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if p.tasks
                .iter()
                .any(|task| task.status == crate::plan::TaskStatus::InReview)
            {
                return Err(PlanError::TransitionDenied {
                    from: crate::plan::TaskStatus::InReview,
                    to: crate::plan::TaskStatus::Cancelled,
                });
            }
            let count = p.tasks.len();
            p.tasks.clear();
            p.heartbeat.current_focus = None;
            p.heartbeat.active_lanes.clear();
            p.mode = crate::plan::PlanMode::Planning;
            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_plan::NewTask;

    fn setup() -> (tempfile::TempDir, PlanStore, String, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let plan = store
            .create_plan(
                "Remove Dependency".into(),
                dir.path().display().to_string(),
                Default::default(),
                vec![],
                1,
            )
            .unwrap();
        let dependency = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "dependency".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let dependent = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "dependent".into(),
                    depends_on: vec![dependency.clone()],
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        (dir, store, plan.id, dependency, dependent)
    }

    #[test]
    fn rm_task_rejects_referenced_dependency_without_mutation() {
        let (_dir, store, plan_id, dependency, dependent) = setup();
        let err = store.rm_task(&plan_id, &dependency, 4).unwrap_err();
        match err {
            PlanError::DependencyTaskReferenced {
                task_id,
                dependent_task_ids,
            } => {
                assert_eq!(task_id, dependency);
                assert_eq!(dependent_task_ids, vec![dependent.clone()]);
            }
            other => panic!("unexpected error: {other}"),
        }
        let plan = store.get(&plan_id).unwrap();
        assert!(plan.tasks.iter().any(|task| task.id == dependency));
        assert_eq!(
            plan.tasks
                .iter()
                .find(|task| task.id == dependent)
                .unwrap()
                .depends_on,
            vec![dependency]
        );
    }

    #[test]
    fn rm_task_succeeds_after_dependency_cleared() {
        let (_dir, store, plan_id, dependency, dependent) = setup();
        store
            .update_task_dependencies(&plan_id, &dependent, vec![], 4)
            .unwrap();
        store.rm_task(&plan_id, &dependency, 5).unwrap();
        let plan = store.get(&plan_id).unwrap();
        assert!(!plan.tasks.iter().any(|task| task.id == dependency));
    }
}
