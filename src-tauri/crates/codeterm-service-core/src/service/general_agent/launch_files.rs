use crate::service::ServiceContext;

use crate::agent::provider::AgentLaunchParams;

pub(super) fn command(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    cwd: &str,
    params: &mut AgentLaunchParams,
    resume: Option<&str>,
) -> Option<String> {
    let resume = resume.map(str::to_owned);
    if let Some(session) = resume.as_deref() {
        crate::service::launch_session_detection::register_resume_launch_marker(tab, session);
    } else {
        crate::service::launch_session_detection::clear_launch_marker(tab);
    }
    let command = if let Some(session) = resume.as_deref() {
        crate::service::task_files::capture(svc, tab, || {
            build_resume(
                &svc.agent_registry,
                provider,
                session,
                params.skip_permissions,
            )
        })
    } else {
        let project = crate::service::launch_session_detection::canonical_project_for_tab(
            &svc.plan_store,
            tab,
            cwd,
        );
        crate::service::launch_session_detection::marked_provider_launch_command_for_service(
            svc,
            &svc.agent_registry,
            provider,
            tab,
            Some(cwd),
            project.as_deref(),
            params,
        )
    }?;
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
            provider.build_resume_command_with_context(&crate::agent::provider::AgentResumeParams {
                session_id: session.to_string(),
                skip_permissions: permission_bypass,
                host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
                ..Default::default()
            })
        })
        .flatten()
}

pub(super) fn general_agent_env_vars(
    tab_id: &str,
    pad_dir: String,
    session_binding_nonce: String,
    config: &crate::config::ConfigState,
    credential: Option<String>,
) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert("CODETERM_TAB_ID".to_string(), tab_id.to_string());
    env.insert("CODETERM_PAD_DIR".to_string(), pad_dir);
    // Per-launch binding nonce so the session-start hook report can prove it owns this tab.
    env.insert(
        "CODETERM_SESSION_BINDING_NONCE".to_string(),
        session_binding_nonce,
    );
    env.insert(
        "CODETERM_SOCKET_PATH".to_string(),
        crate::paths::socket_display_path(),
    );
    env.insert(
        "CODETERM_MACHINE_AGENT_IDENTITY".to_string(),
        "general_agent".to_string(),
    );
    // Derive URL+port from the daemon config, not a parent CodeTerm env leak —
    // an inherited prod CODETERM_API_URL (7685) 401s this instance's token.
    let (api_url, api_port) = crate::config::api_callback_url(&config.lock().api);
    env.insert("CODETERM_API_URL".to_string(), api_url);
    env.insert("CODETERM_API_PORT".to_string(), api_port);
    if let Some(credential) = credential {
        env.insert("CODETERM_API_TOKEN".into(), credential);
    }
    env
}

#[cfg(test)]
mod tests;
