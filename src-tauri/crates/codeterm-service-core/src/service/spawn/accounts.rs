use super::{AgentSpawnSpec, ServiceContext, SpawnTabSpec, TabCommand};
use codeterm_plugin::provider_account::InvocationContext;
use std::sync::Arc;

pub(super) fn prepare(
    svc: &ServiceContext,
    spec: &mut SpawnTabSpec,
) -> Result<Option<Arc<InvocationContext>>, String> {
    let agent: AgentSpawnSpec = match &spec.command {
        TabCommand::Agent(agent) => agent.clone(),
        TabCommand::ChatBackend { .. } => match &spec.chatbackend_launch {
            Some(metadata) => metadata.agent.clone(),
            None => return Ok(None),
        },
        TabCommand::Shell => return Ok(None),
    };
    let provider = if agent.provider.is_empty() {
        svc.configuration().general_agent.agent_provider
    } else {
        agent.provider.clone()
    };
    let workspace = {
        let mux = svc.mux.read();
        super::resolve_spawn_workspace_id(
            spec.workspace_id.clone(),
            spec.parent_tab_id.as_deref(),
            super::spawn_caller_tab_id(&agent, spec),
            |tab| mux.find_workspace_for_tab(tab),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )?
    };
    let account = crate::service::provider_accounts::starting_account(
        svc,
        &provider,
        Some(&workspace),
        agent.account_id.as_deref(),
    )?;
    let context =
        crate::service::provider_accounts::invocation(svc, &provider, account.as_deref())?;
    if let TabCommand::Agent(agent) = &mut spec.command {
        agent.provider = provider.clone();
        agent.account_id = account.clone();
    }
    if let Some(metadata) = &mut spec.chatbackend_launch {
        metadata.agent.provider = provider;
        metadata.agent.account_id = account.clone();
        metadata.params.account_id = account;
    }
    spec.workspace_id = Some(workspace);
    Ok(context)
}
