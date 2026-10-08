use super::lifecycle::{Sidecar, Spawn};
use crate::service::{ServiceContext, SpawnAgentParams};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Default)]
pub(crate) struct Selection {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub resume: Option<String>,
    pub extra_args: bool,
}

impl Selection {
    pub fn spawn(
        self,
        provider: &str,
        cwd: &str,
        instructions: Option<String>,
        full_access: bool,
    ) -> Spawn {
        Spawn {
            provider: provider.into(),
            cwd: cwd.into(),
            instructions,
            resume: self.resume,
            model: self.model,
            effort: self.effort,
            full_access,
        }
    }
}

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

fn selected(svc: &ServiceContext, tab: &str, provider: &str, extra_args: bool) -> bool {
    if !requested(svc, provider) {
        return false;
    }
    if extra_args {
        super::diagnostics::fallback(svc, tab, provider, "unsupported extra terminal arguments");
        return false;
    }
    true
}

pub(crate) fn selection_plan(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    cwd: &str,
    params: &crate::agent::provider::AgentLaunchParams,
    selection: Selection,
) -> Option<Spawn> {
    if !selected(svc, tab, provider, selection.extra_args) {
        return None;
    }
    let instructions = svc
        .agent_registry
        .with_provider(provider, |provider| {
            crate::service::launch_delivery::command_task(provider, params.task.as_deref())
                .system_prompt
        })
        .flatten();
    Some(selection.spawn(provider, cwd, instructions, params.skip_permissions))
}

pub(crate) fn prepare_selection(svc: &ServiceContext, tab: &str, spawn: Spawn) -> Option<String> {
    let provider = spawn.provider.clone();
    match tauri::async_runtime::block_on(super::lifecycle::ensure_for_spawn(svc, tab, spawn)) {
        Ok(sidecar) => Some(remote_command(&sidecar)),
        Err(error) => {
            log::error!("[codex-rpc] launch tab={tab} fallback: {error}");
            super::diagnostics::fallback(svc, tab, &provider, &error);
            None
        }
    }
}

pub fn defer(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    params: &SpawnAgentParams,
    task: Option<&str>,
    injected: Option<&str>,
) -> Option<String> {
    if !selected(
        svc,
        tab,
        provider,
        params.args.as_ref().is_some_and(|args| !args.is_empty()),
    ) {
        return None;
    }
    super::delivery::begin(tab, &svc.ensure_session_binding_nonce(tab));
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
        if super::lifecycle::for_tab(tab).is_none() {
            super::lifecycle::plain_launch(svc, tab, &svc.ensure_session_binding_nonce(tab));
            if let Some(tab) = svc.mux.write().get_tab_mut(tab) {
                tab.structured_control_active = false;
            }
        }
        return command;
    }
    let Some(plan) = PLANS.lock().remove(tab) else {
        super::diagnostics::fallback(svc, tab, provider, "deferred launch plan missing");
        if let Some(sidecar) = super::lifecycle::for_tab(tab) {
            return remote_command(&sidecar);
        }
        super::lifecycle::plain_launch(svc, tab, &svc.ensure_session_binding_nonce(tab));
        let params = svc
            .mux
            .read()
            .get_tab(tab)
            .map(|tab| SpawnAgentParams {
                agent: Some(provider.into()),
                cwd: tab.cwd.clone(),
                resume_session_id: tab.agent_session_id.clone(),
                model: tab.restorable_launch.model.clone(),
                reasoning_effort: tab.restorable_launch.resolved_reasoning_effort.clone(),
                ..Default::default()
            })
            .unwrap_or_default();
        return crate::service::orchestrator::build_agent_command(
            provider,
            &params,
            None,
            None,
            &svc.agent_registry,
        );
    };
    if super::lifecycle::for_tab(tab).is_none() {
        svc.rotate_session_binding_nonce(tab);
    }
    let launch_nonce = svc.ensure_session_binding_nonce(tab);
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
            if let Some(sidecar) = super::lifecycle::for_tab(tab) {
                log::error!("[codex-rpc] tab={tab} retained sidecar refused replacement: {error}");
                return remote_command(&sidecar);
            }
            log::error!("[codex-rpc] tab={tab} sidecar unavailable, plain PTY fallback: {error}");
            super::lifecycle::plain_launch(svc, tab, &launch_nonce);
            super::diagnostics::fallback(svc, tab, provider, &error);
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
    format!("{prefix}{} -c check_for_update_on_startup=false --remote ws://127.0.0.1:{} --remote-auth-token-env CODETERM_CODEX_RPC_TOKEN resume {}",
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
        assert!(command.contains("-c check_for_update_on_startup=false"));
    }
}
#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn codex_rpc_control_hidden_selection_preserves_model_effort_and_resume() {
        let selection = Selection {
            model: Some("selected".into()),
            effort: Some("effort".into()),
            resume: Some("own".into()),
            extra_args: false,
        };
        let spawn = selection.spawn("provider", "cwd", Some("instructions".into()), true);
        assert_eq!(spawn.model.as_deref(), Some("selected"));
        assert_eq!(spawn.effort.as_deref(), Some("effort"));
        assert_eq!(spawn.resume.as_deref(), Some("own"));
        assert!(spawn.full_access);
    }
}

pub fn clear_tab(tab: &str) {
    PLANS.lock().remove(tab);
}

#[cfg(test)]
pub(crate) fn deferred_params(tab: &str) -> Option<SpawnAgentParams> {
    PLANS.lock().get(tab).map(|plan| plan.params.clone())
}
