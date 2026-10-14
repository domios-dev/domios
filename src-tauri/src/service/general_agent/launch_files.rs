use crate::service::ServiceContext;

use crate::agent::provider::{AgentLaunchParams, StarterPrompt};
use crate::service::codex_rpc;

#[cfg(test)]
pub(super) fn plan(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    cwd: &str,
    params: &AgentLaunchParams,
    resume: Option<&str>,
) -> Option<codex_rpc::lifecycle::Spawn> {
    codex_rpc::launch::selection_plan(
        svc,
        tab,
        provider,
        cwd,
        params,
        codex_rpc::launch::Selection {
            resume: resume.map(str::to_owned),
            extra_args: params.args.as_ref().is_some_and(|args| !args.is_empty()),
            ..Default::default()
        },
    )
}

pub(super) fn command(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    cwd: &str,
    params: &mut AgentLaunchParams,
    resume: Option<&str>,
) -> Option<String> {
    let selection = codex_rpc::launch::Selection {
        resume: resume.map(str::to_owned),
        extra_args: params.args.as_ref().is_some_and(|args| !args.is_empty()),
        ..Default::default()
    };
    command_with_selection(svc, tab, provider, cwd, params, selection)
}

pub(super) fn command_with_selection(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    cwd: &str,
    params: &mut AgentLaunchParams,
    selection: codex_rpc::launch::Selection,
) -> Option<String> {
    let resume = selection.resume.clone();
    if let Some(session) = resume.as_deref() {
        crate::service::launch_session_detection::register_resume_launch_marker(tab, session);
    } else {
        crate::service::launch_session_detection::clear_launch_marker(tab);
    }
    if let Some(command) =
        codex_rpc::launch::selection_plan(svc, tab, provider, cwd, params, selection)
            .and_then(|plan| codex_rpc::launch::prepare_selection(svc, tab, plan))
    {
        return Some(command);
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
    Some(codex_rpc::launch::prepare(svc, tab, provider, command))
}

pub(super) fn starter(
    svc: &ServiceContext,
    provider: &str,
    prompt: StarterPrompt,
) -> Option<String> {
    if prompt == StarterPrompt::NoStarter {
        return None;
    }
    let text = svc
        .agent_registry
        .with_provider(provider, |provider| provider.starter_prompt_text(prompt))
        .flatten();
    let text = crate::service::launch_delivery::assigned_task_starter(text.as_deref());
    Some(crate::service::launch_delivery::mark_starter_prompt(
        prompt, text,
    ))
}

pub(super) fn finish_structured(
    svc: &ServiceContext,
    tab: &str,
    pty: &str,
    payload: Option<&str>,
) -> Result<bool, String> {
    let Some(sidecar) = codex_rpc::lifecycle::for_tab(tab) else {
        return Ok(false);
    };
    let nonce = &sidecar.launch_nonce;
    if !codex_rpc::lifecycle::wait_attached(tab, nonce, std::time::Duration::from_secs(30)) {
        codex_rpc::recovery::try_start(svc, tab, nonce, "general remote TUI attachment failed");
        if !codex_rpc::recovery::wait_recovery(tab, nonce) {
            return Err("General Agent structured attachment failed".into());
        }
    }
    let attempt = crate::service::launch_readiness::LaunchAttempt::current(&svc.mux, tab, pty)
        .ok_or("General Agent launch was superseded")?;
    if let Some(payload) = payload {
        let consumed = codex_rpc::bootstrap::consume(
            svc,
            &attempt,
            crate::service::launch_delivery::FallbackDeliverySpec {
                channel: crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                payload: payload.into(),
            },
        )?;
        if !consumed {
            return Err("General Agent structured startup was not confirmed".into());
        }
    }
    codex_rpc::lifecycle::startup_complete(svc, tab, nonce);
    Ok(true)
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

pub(super) fn general_agent_env_vars(
    tab_id: &str,
    pad_dir: String,
    session_binding_nonce: String,
    config: &crate::config::ConfigState,
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
    // Hook callbacks POST to our REST API. Without the token they'd 401 on any non-loopback bind and silently swallow the failure via `curl … || true`.
    let tok: Option<String> = config.lock().api.token.clone();
    if let Some(tok) = tok {
        if !tok.is_empty() {
            env.insert("CODETERM_API_TOKEN".to_string(), tok);
        }
    }
    env
}

#[cfg(test)]
mod tests;
