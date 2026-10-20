use super::orchestrator::SpawnAgentParams;
use super::ServiceContext;
use crate::agent::provider::WorkerLaneSetting;
pub use codeterm_lanes::worker_lane::*;
impl ServiceContext {
    pub fn provision_worker_lane_for_spawn(
        &self,
        params: &mut SpawnAgentParams,
        lane_label: &str,
    ) -> Result<Option<ProvisionedWorkerLane>, String> {
        if !is_worker_spawn(params.role.as_deref(), params.role_profile.as_deref()) {
            return Ok(None);
        }
        let workspace_cwd = self.resolve_worker_spawn_workspace_cwd(params);
        let effective_cwd = params.cwd.clone().or_else(|| workspace_cwd.clone());
        let Some(cwd) = effective_cwd else {
            return Ok(None);
        };
        if !is_canonical_checkout(&cwd, workspace_cwd.as_deref()) {
            return Ok(None);
        }

        let lane = provision_lane(&cwd, lane_label, &self.worker_lane_settings())?;
        params.cwd = Some(lane.cwd.clone());
        params.provisioned_cwd = Some(lane.cwd.clone());
        Ok(Some(lane))
    }

    pub fn resolve_spawn_workspace_identity(
        &self,
        params: &SpawnAgentParams,
    ) -> Option<(String, String)> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_tab_id.as_deref(),
            crate::service::spawn::params_caller_tab_id(params),
            |tab_id| mux.find_workspace_for_tab(tab_id),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .ok()?;
        mux.get_workspace(&id)
            .map(|workspace| (workspace.id.clone(), workspace.name.clone()))
    }

    /// Lane settings every registered provider declares; core never names a provider here.
    pub fn worker_lane_settings(&self) -> Vec<WorkerLaneSetting> {
        self.agent_registry
            .all_provider_arcs()
            .iter()
            .flat_map(|provider| provider.worker_lane_settings())
            .collect()
    }

    pub(crate) fn worker_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        self.resolve_worker_spawn_workspace_cwd(params)
    }

    /// Dispatcher-owned provisioning. Unlike `provision_worker_lane_for_spawn`
    /// it never no-ops: the dispatcher has already decided this checkout is the
    /// base to branch from, so inferring that again could only disagree.
    pub fn provision_dispatch_worker_lane(
        &self,
        params: &mut SpawnAgentParams,
        base_checkout: &str,
        lane_label: &str,
    ) -> Result<ProvisionedWorkerLane, String> {
        let lane = provision_lane(base_checkout, lane_label, &self.worker_lane_settings())?;
        params.cwd = Some(lane.cwd.clone());
        params.provisioned_cwd = Some(lane.cwd.clone());
        Ok(lane)
    }

    fn resolve_worker_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_tab_id.as_deref(),
            crate::service::spawn::params_caller_tab_id(params),
            |tab_id| mux.find_workspace_for_tab(tab_id),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .ok()?;
        mux.get_workspace(&id)
            .and_then(|workspace| workspace.cwd.clone())
    }
}
