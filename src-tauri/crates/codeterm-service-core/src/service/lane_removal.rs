use super::ServiceContext;
pub use codeterm_lanes::lane_removal::*;
pub use codeterm_lanes::residue;
impl ServiceContext {
    pub fn cleanup_closed_task_lane(
        &self,
        plan: &crate::plan::Plan,
        task_id: &str,
        now_ms: u64,
    ) -> crate::service::plan::lane_cleanup::LaneCleanupOutcome {
        use crate::service::plan::lane_cleanup::{try_cleanup_task_lane, LaneCleanupOutcome};
        let task = plan.tasks.iter().find(|task| task.id == task_id);
        let path = task.and_then(|task| task.lane.worktree_path.as_deref());
        let existed = path.is_some_and(|path| std::path::Path::new(path).exists());
        let tabs = self.live_tab_cwds();
        if let Some(holder) = path.and_then(|path| find_lane_holder(path, &tabs)) {
            return LaneCleanupOutcome::Refused(format!(
                "worktree '{}' held by tab {} ({})",
                path.unwrap_or_default(),
                holder.id,
                holder.title
            ));
        }
        let outcome = try_cleanup_task_lane(
            &self.plan_store,
            plan,
            task_id,
            |tab| self.tab_probe(tab).exists,
            now_ms,
        );
        if let Some(path) = path.filter(|path| existed && !std::path::Path::new(path).exists()) {
            if let Err(error) = finalize_lane_removal(
                &self.plan_store,
                self.ext::<crate::repowatch::RepoWatchRegistryHandle>(),
                &LaneRemovalOutcome {
                    canonical_cwd: crate::service::lane_settle::canonical_path(plan).into(),
                    worktree_path: path.into(),
                    branch: task.and_then(|task| task.lane.branch.clone()),
                    worktree_already_removed: false,
                    branch_already_removed: false,
                },
            ) {
                return LaneCleanupOutcome::Partial(error);
            }
        }
        outcome
    }

    pub fn live_tab_cwds(&self) -> Vec<LiveTabCwd> {
        self.mux
            .read()
            .all_tabs()
            .filter_map(|tab| {
                let cwd = tab.cwd.clone()?;
                (!cwd.trim().is_empty()).then(|| LiveTabCwd {
                    id: tab.id.clone(),
                    title: tab.title.clone(),
                    cwd,
                })
            })
            .collect()
    }

    pub fn remove_owned_lane(
        &self,
        request: &LaneRemovalRequest,
    ) -> Result<LaneRemovalOutcome, String> {
        remove_lane_and_clear_task_references(
            &self.plan_store,
            self.ext::<crate::repowatch::RepoWatchRegistryHandle>(),
            request,
            &self.live_tab_cwds(),
        )
    }
}
