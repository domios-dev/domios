use super::{PlanError, PlanStore};
use crate::plan::TaskFiles;

impl PlanStore {
    pub fn update_task_files(
        &self,
        plan_id: &str,
        task_id: &str,
        files: TaskFiles,
        now: u64,
    ) -> Result<(), PlanError> {
        let files = files.validated().map_err(PlanError::InvalidPlanText)?;
        self.with_plan(plan_id, now, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.files = files;
            task.updated_at = now;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service_plan::NewTask;
    #[test]
    fn task_files_persist_creation_and_replacement_and_refuse_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "opaque".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut plan, 1).unwrap();
        let first = TaskFiles {
            edit: vec!["new.rs".into()],
            ..Default::default()
        };
        let id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "opaque".into(),
                    files: first.clone(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        assert_eq!(store.show_task(&id).unwrap().task.files, first);
        let second = TaskFiles {
            read: vec!["read.md".into()],
            ..Default::default()
        };
        store
            .update_task_files(&plan.id, &id, second.clone(), 3)
            .unwrap();
        assert!(store
            .update_task_files(
                &plan.id,
                &id,
                TaskFiles {
                    edit: vec!["x".into()],
                    read: vec!["x".into()]
                },
                4
            )
            .is_err());
        assert_eq!(store.show_task(&id).unwrap().task.files, second);
    }
}
