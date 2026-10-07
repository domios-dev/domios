use crate::service::ServiceContext;

pub(super) fn resume(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    session: &str,
    permission_bypass: bool,
) -> Option<String> {
    let command = crate::service::task_files::capture(svc, tab, || {
        build_resume(&svc.agent_registry, provider, session, permission_bypass)
    })?;
    Some(command)
}

pub(super) fn build_resume(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session: &str,
    permission_bypass: bool,
) -> Option<String> {
    registry
        .with_provider(provider, |provider| {
            provider.build_resume_command(session, permission_bypass)
        })
        .flatten()
}
