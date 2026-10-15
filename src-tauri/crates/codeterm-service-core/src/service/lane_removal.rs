use super::ServiceContext;
pub use codeterm_lanes::lane_removal::*;
impl ServiceContext {
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
