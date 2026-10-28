use super::*;
use crate::{
    multiplexer::types::NeighborAuthority,
    service::{
        spawn::{SpawnTabSpec, TabCommand},
        SpawnAgentParams,
    },
};

const DENIED: &str = "neighbor_capability_denied: This launch cannot preserve the friend's granted workspace and machine access. Politely refuse the current request through the Neighbor channel with --kind refusal and --in-reply-to set to its message_id.";

impl ServiceContext {
    pub(super) fn require_neighbor_spawn_workspace(
        &self,
        permission: &Permission,
        selector: Option<&str>,
    ) -> Result<String, String> {
        Capability::Spawn.require(permission)?;
        let workspace = crate::dispatch::resolve_workspace_selector(
            &self.mux.read().snapshot(),
            selector.ok_or(DENIED)?,
        )
        .map_err(|_| DENIED)?;
        if permission
            .workspaces
            .as_ref()
            .is_some_and(|ids| ids.contains(&workspace))
        {
            Ok(workspace)
        } else {
            Err(DENIED.into())
        }
    }
    pub(crate) fn neighbor_tab_launch_guard(
        &self,
        spec: &SpawnTabSpec,
    ) -> Result<Option<parking_lot::MutexGuard<'_, ()>>, String> {
        let spawner = match &spec.command {
            TabCommand::Agent(agent) => agent.spawned_by_tab_id.as_deref(),
            _ => spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.params.spawned_by_tab_id.as_deref()),
        };
        if self
            .neighbor_launch_caller(&[
                spawner,
                spec.creator_tab_id.as_deref(),
                spec.parent_tab_id.as_deref(),
            ])?
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(self.neighbor_cell().launches.gate.lock()))
    }
    pub(crate) fn inherit_neighbor_agent_spawn_sync(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<bool, String> {
        if self
            .neighbor_launch_caller(&[
                params.spawned_by_tab_id.as_deref(),
                params.creator_tab_id.as_deref(),
                params.parent_tab_id.as_deref(),
            ])?
            .is_none()
        {
            return Ok(false);
        }
        crate::async_runtime::block_on(self.inherit_neighbor_agent_spawn(params))
    }
    fn neighbor_launch_caller(
        &self,
        callers: &[Option<&str>],
    ) -> Result<Option<TabCaller>, String> {
        let mux = self.mux.read();
        let known = self.neighbor_cell().concierge_accounts.lock();
        for id in callers.iter().flatten() {
            if let Some(tab) = mux.get_tab(id) {
                if tab.neighbor_authority.is_some() || known.contains_key(*id) {
                    return Ok(Some(TabCaller {
                        tab_id: tab.id.clone(),
                        binding_nonce: tab.session_binding_nonce.clone().ok_or(DENIED)?,
                    }));
                }
            } else if known.contains_key(*id) {
                return Err(DENIED.into());
            }
        }
        Ok(None)
    }

    pub async fn inherit_neighbor_agent_spawn(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<bool, String> {
        let Some(caller) = self.neighbor_launch_caller(&[
            params.spawned_by_tab_id.as_deref(),
            params.creator_tab_id.as_deref(),
            params.parent_tab_id.as_deref(),
        ])?
        else {
            return Ok(false);
        };
        let Some((friendship, permission)) = self.neighbor_capability_permission(&caller).await?
        else {
            return Err(DENIED.into());
        };
        Capability::Spawn.require(&permission)?;
        if self.neighbor_launch_blocked(&friendship) {
            return Err(DENIED.into());
        }
        let workspace =
            self.require_neighbor_spawn_workspace(&permission, params.workspace_id.as_deref())?;
        if params.args.as_ref().is_some_and(|args| !args.is_empty())
            || params.preset.is_some()
            || params.execution_mode.is_some()
            || params.resume_session_id.is_some()
            || params.fork_session
            || params.existing_tab_id.is_some()
            || params.plan_id.is_some()
            || params.task_id.is_some()
            || params.plan_task.is_some()
            || params.remote_task_binding.is_some()
            || params.report_target_mesh_peer.is_some()
            || params.ownership.is_some()
            || params.team_id.is_some()
            || params.role.as_deref().is_some_and(|role| role != "Worker")
            || params.role_profile.as_deref().is_some_and(|profile| {
                profile != codeterm_types::agent::role_kind::builtins::WORKER
            })
            || params
                .parent_tab_id
                .as_deref()
                .is_some_and(|parent| parent != caller.tab_id)
        {
            return Err(DENIED.into());
        }
        let (authority, provider) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(&caller.tab_id).ok_or(DENIED)?;
            let authority = tab.neighbor_authority.clone().ok_or(DENIED)?;
            let root = authority.root_tab_id.as_deref().unwrap_or(&caller.tab_id);
            let provider = params
                .agent
                .clone()
                .filter(|id| !id.is_empty())
                .or_else(|| mux.get_tab(root).and_then(|tab| tab.agent_type.clone()))
                .ok_or(DENIED)?;
            (authority, provider)
        };
        let child_authority = NeighborAuthority {
            uid: authority.uid.clone(),
            friendship_id: friendship.clone(),
            root_tab_id: Some(
                authority
                    .root_tab_id
                    .unwrap_or_else(|| caller.tab_id.clone()),
            ),
        };
        if params
            .neighbor_authority
            .as_ref()
            .is_some_and(|existing| existing != &child_authority)
        {
            return Err(DENIED.into());
        }
        let metadata = Metadata::new(self.account_data_root()?, &authority.uid);
        let cwd =
            super::super::confinement::prepare_folder(&metadata, &friendship, &permission).await?;
        let sandbox = self.neighbor_concierge_sandbox(&provider, permission.machine_access)?;
        self.neighbor_capability_permission(&caller)
            .await?
            .filter(|(current_friend, current)| {
                current_friend == &friendship && current == &permission
            })
            .ok_or(DENIED)?;
        params.agent = Some(provider);
        params.workspace_id = Some(workspace);
        params.cwd = Some(cwd);
        params.sandbox = sandbox;
        params.execution_intent = None;
        params.skip_permissions = Some(false);
        params.permission_bypass_authorization = None;
        params.neighbor_authority = Some(child_authority);
        params.spawned_by_tab_id = Some(caller.tab_id.clone());
        params.creator_tab_id = Some(caller.tab_id.clone());
        params.parent_tab_id = Some(caller.tab_id.clone());
        params.report_target_tab_id = Some(caller.tab_id);
        crate::service::execution_sandbox::enforce_delegation_ceiling(self, params, None)
            .map_err(|_| DENIED)?;
        Ok(true)
    }

    pub(crate) fn prepare_neighbor_tab_spawn(&self, spec: &mut SpawnTabSpec) -> Result<(), String> {
        let spawner = match &spec.command {
            TabCommand::Agent(agent) => agent.spawned_by_tab_id.as_deref(),
            _ => spec
                .chatbackend_launch
                .as_ref()
                .and_then(|launch| launch.params.spawned_by_tab_id.as_deref()),
        };
        if self
            .neighbor_launch_caller(&[
                spawner,
                spec.creator_tab_id.as_deref(),
                spec.parent_tab_id.as_deref(),
            ])?
            .is_none()
        {
            return Ok(());
        }
        if spec.cmd.is_some() || spec.shell.is_some() {
            return Err(DENIED.into());
        }
        let mut params = match &spec.command {
            TabCommand::Agent(_) => match spec.chatbackend_launch.as_ref() {
                Some(launch) => launch.params.clone(),
                None => spec.to_agent_params()?,
            },
            TabCommand::ChatBackend { .. } => spec
                .chatbackend_launch
                .as_ref()
                .map(|launch| launch.params.clone())
                .ok_or(DENIED)?,
            TabCommand::Shell => return Err(DENIED.into()),
        };
        if !crate::async_runtime::block_on(self.inherit_neighbor_agent_spawn(&mut params))? {
            return Err(DENIED.into());
        }
        let mut prepared = SpawnTabSpec::from_agent_params_with_chatbackends(
            self.chatbackend_registry.as_ref(),
            params,
        );
        prepared.visibility = spec.visibility;
        prepared.audience = spec.audience;
        prepared.icon = spec.icon.clone();
        *spec = prepared;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
