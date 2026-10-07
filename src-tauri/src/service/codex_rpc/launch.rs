use super::lifecycle::{Sidecar, Spawn};
use crate::service::{ServiceContext, SpawnAgentParams};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

const DEFERRED: &str = "__CODETERM_CODEX_RPC_LAUNCH__";
struct Plan {
    params: SpawnAgentParams,
    task: Option<String>,
    injected: Option<String>,
}
static PLANS: LazyLock<Mutex<HashMap<String, Plan>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn requested(svc: &ServiceContext, provider: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |provider| {
            super::capability::enabled(
                provider.structured_control().as_ref(),
                &crate::config::provider_config(provider.id()),
            )
        })
        .unwrap_or(false)
}

pub fn defer(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected: Option<&str>,
) -> Option<String> {
    if !requested(svc, provider) {
        return None;
    }
    PLANS.lock().insert(
        tab.into(),
        Plan {
            params: params.clone(),
            task: task.map(str::to_owned),
            injected: injected.map(str::to_owned),
        },
    );
    Some(DEFERRED.into())
}

pub fn prepare(svc: &ServiceContext, tab: &str, provider: &str, command: String) -> String {
    if command != DEFERRED {
        return command;
    }
    let Some(plan) = PLANS.lock().remove(tab) else {
        return String::new();
    };
    svc.rotate_session_binding_nonce(tab);
    let instructions = svc
        .agent_registry
        .with_provider(provider, |p| {
            crate::service::launch_delivery::command_task(p, plan.task.as_deref()).system_prompt
        })
        .flatten();
    let spawn = Spawn {
        provider: provider.into(),
        cwd: plan.params.cwd.clone().unwrap_or_default(),
        instructions,
        resume: plan.params.resume_session_id.clone(),
        model: plan.params.model.clone(),
        effort: plan.params.reasoning_effort.clone(),
        full_access: plan.params.permission_bypass_requested(provider),
    };
    match tauri::async_runtime::block_on(super::lifecycle::ensure_for_spawn(svc, tab, spawn)) {
        Ok(sidecar) => remote_command(&sidecar),
        Err(error) => {
            super::lifecycle::clear_tab(tab);
            log::error!("[codex-rpc] tab={tab} sidecar unavailable, plain PTY fallback: {error}");
            svc.notify_model_resolution_warning(
                tab,
                provider,
                &format!("Structured control unavailable; using the plain terminal: {error}"),
            );
            crate::service::orchestrator::build_agent_command(
                provider,
                &plan.params,
                plan.task.as_deref(),
                plan.injected.as_deref(),
                &svc.agent_registry,
            )
        }
    }
}

pub fn remote_command(sidecar: &Sidecar) -> String {
    remote_command_parts(&sidecar.binary, sidecar.port, &sidecar.thread.lock())
}

fn remote_command_parts(binary: &std::path::Path, port: u16, thread: &str) -> String {
    let quote = codeterm_util::shell::ShellSyntax::quote;
    let prefix = if cfg!(windows) { "& " } else { "" };
    format!("{prefix}{} --remote ws://127.0.0.1:{} --remote-auth-token-env CODETERM_CODEX_RPC_TOKEN resume {}",
        quote(&binary.to_string_lossy()), port, quote(thread))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_launch_uses_absolute_binary_and_token_env_name_only() {
        let binary = if cfg!(windows) {
            std::path::Path::new("C:/Program Files/Codex/codex.exe")
        } else {
            std::path::Path::new("/opt/codex/bin/codex")
        };
        let command = remote_command_parts(binary, 19001, "thread-id");
        assert!(command.contains(&codeterm_util::shell::ShellSyntax::quote(
            &binary.to_string_lossy()
        )));
        assert!(command.contains(
            "--remote ws://127.0.0.1:19001 --remote-auth-token-env CODETERM_CODEX_RPC_TOKEN resume"
        ));
        assert!(!command.contains("--profile"));
    }
}

pub fn clear_tab(tab: &str) {
    PLANS.lock().remove(tab);
}
