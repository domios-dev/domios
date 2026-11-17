use super::{AgentSpawnSpec, ServiceContext, SpawnTabSpec, TabCommand};
use codeterm_plugin::provider_account::InvocationContext;
use std::sync::Arc;

pub(super) fn prepare(
    svc: &ServiceContext,
    spec: &mut SpawnTabSpec,
) -> Result<
    (
        Option<Arc<InvocationContext>>,
        Option<crate::service::provider_accounts::admission::Reservation>,
    ),
    String,
> {
    let agent: AgentSpawnSpec = match &spec.command {
        TabCommand::Agent(agent) => agent.clone(),
        TabCommand::ChatBackend { .. } => match &spec.chatbackend_launch {
            Some(metadata) => metadata.agent.clone(),
            None => return Ok((None, None)),
        },
        TabCommand::Shell => return Ok((None, None)),
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
    let namespace = crate::service::provider_accounts::credential_namespace(
        svc,
        &provider,
        account.as_deref(),
        agent.account_namespace.as_deref(),
    )?;
    let reservation =
        crate::service::provider_accounts::admission::reserve(svc, &provider, account.as_deref())?;
    let context = crate::service::provider_accounts::invocation_at(
        svc,
        &provider,
        account.as_deref(),
        namespace.as_deref(),
    )?;
    if let TabCommand::Agent(agent) = &mut spec.command {
        agent.provider = provider.clone();
        agent.account_id = account.clone();
        agent.account_namespace = namespace.clone();
    }
    if let Some(metadata) = &mut spec.chatbackend_launch {
        metadata.agent.provider = provider;
        metadata.agent.account_id = account.clone();
        metadata.agent.account_namespace = namespace.clone();
        metadata.params.account_namespace = namespace;
        metadata.params.account_id = account;
    }
    spec.workspace_id = Some(workspace);
    Ok((context, Some(reservation)))
}
