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
    let plan = match PLANS.lock().remove(tab) {
        Some(plan) => plan,
        None => {
            if let Some(sidecar) = super::lifecycle::for_tab(tab) {
                return remote_command(&sidecar);
            }
            // A restart can clear the plan between defer and prepare; rebuild it from the tab
            // instead of dropping to the plain PTY lane.
            log::warn!(
                "[codex-rpc] tab={tab} deferred launch plan missing; rebuilding it from the tab"
            );
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
            Plan {
                params,
                task: None,
                injected: None,
            }
        }
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
    let spawn = deferred_spawn(provider, &plan.params, instructions);
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

fn deferred_spawn(
    provider: &str,
    params: &SpawnAgentParams,
    instructions: Option<String>,
) -> Spawn {
    Spawn {
        provider: provider.into(),
        cwd: params.cwd.clone().unwrap_or_default(),
        instructions,
        resume: params.resume_session_id.clone(),
        model: params.model.clone(),
        effort: params.reasoning_effort.clone(),
        full_access: params.permission_bypass_requested(provider),
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
    fn codex_rpc_launch_spawn_origins_use_resolved_cwd() {
        use crate::service::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand};

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_string_lossy().into_owned();
        let explicit = dir.path().join("explicit");
        std::fs::create_dir(&explicit).unwrap();
        let explicit = explicit.to_string_lossy().into_owned();
        for (origin, requested, task, role, expected) in [
            ("visible_spawn", None, None, None, &workspace),
            (
                "visible_spawn_cwd",
                Some(explicit.clone()),
                None,
                None,
                &explicit,
            ),
            (
                "visible_spawn_task_file",
                None,
                Some("opaque file contents"),
                None,
                &workspace,
            ),
            (
                "visible_spawn_task_file_cwd",
                Some(explicit.clone()),
                Some("opaque file contents"),
                None,
                &explicit,
            ),
            (
                "plan_dispatch",
                Some(explicit.clone()),
                Some("opaque brief"),
                Some("Worker"),
                &explicit,
            ),
        ] {
            let spec = SpawnTabSpec {
                shell: None,
                cwd: requested,
                command: TabCommand::Agent(AgentSpawnSpec {
                    provider: "provider".into(),
                    task: task.map(str::to_owned),
                    role: role.map(str::to_owned),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (resolved, _) =
                ServiceContext::resolve_agent_cwd(&spec.cwd, &Some(workspace.clone()));
            let params = spec.to_agent_launch_params(resolved).unwrap();
            let spawn = deferred_spawn("provider", &params, task.map(str::to_owned));
            assert_eq!(&spawn.cwd, expected, "{origin}");
            assert_eq!(
                crate::service::spawn_cwd::cwd_problem(&spawn.cwd),
                None,
                "{origin}"
            );
            assert_eq!(spawn.instructions.as_deref(), task, "{origin}");
        }
        let ui = Selection::default().spawn("provider", &workspace, None, false);
        assert_eq!(crate::service::spawn_cwd::cwd_problem(&ui.cwd), None);
    }

    #[test]
    fn codex_rpc_launch_without_cwd_pins_daemon_directory() {
        let spec = crate::service::spawn::SpawnTabSpec {
            shell: None,
            command: crate::service::spawn::TabCommand::Agent(Default::default()),
            ..Default::default()
        };
        let (resolved, _) = ServiceContext::resolve_agent_cwd(&None, &None);
        let params = spec.to_agent_launch_params(resolved).unwrap();
        let spawn = deferred_spawn("provider", &params, None);
        assert_eq!(crate::service::spawn_cwd::cwd_problem(&spawn.cwd), None);
        assert_eq!(
            std::path::Path::new(&spawn.cwd),
            std::env::current_dir().unwrap()
        );
    }

    #[test]
    #[cfg(windows)]
    fn codex_rpc_launch_reproduces_os_error_123_and_resolves_long_prompt_cwd() {
        let binary = std::env::current_exe().unwrap();
        let command = || {
            let mut command = std::process::Command::new(&binary);
            crate::process_util::no_window_std(&mut command);
            command.args(["--list", "--exact", "opaque-filter"]);
            command
        };
        let error = command().current_dir("").output().unwrap_err();
        assert_eq!(error.raw_os_error(), Some(123));

        let dir = tempfile::tempdir().unwrap();
        let prompt = "opaque:\n<>|?*\"\n".repeat(500);
        let spec = crate::service::spawn::SpawnTabSpec {
            command: crate::service::spawn::TabCommand::Agent(
                crate::service::spawn::AgentSpawnSpec {
                    task: Some(prompt.clone()),
                    ..Default::default()
                },
            ),
            ..Default::default()
        };
        let (resolved, _) = ServiceContext::resolve_agent_cwd(
            &None,
            &Some(dir.path().to_string_lossy().into_owned()),
        );
        let params = spec.to_agent_launch_params(resolved).unwrap();
        let spawn = deferred_spawn("provider", &params, Some(prompt.clone()));
        assert_eq!(spawn.instructions.as_deref(), Some(prompt.as_str()));
        assert_eq!(std::path::Path::new(&spawn.cwd), dir.path());
        let output = command().current_dir(&spawn.cwd).output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    #[cfg(windows)]
    fn codex_rpc_launch_normalizes_verbatim_workspace_before_deferral() {
        let spec = crate::service::spawn::SpawnTabSpec {
            shell: None,
            command: crate::service::spawn::TabCommand::Agent(Default::default()),
            ..Default::default()
        };
        let (resolved, _) =
            ServiceContext::resolve_agent_cwd(&None, &Some(r"\\?\D:\Developer\codeterm".into()));
        let params = spec.to_agent_launch_params(resolved).unwrap();
        assert_eq!(
            deferred_spawn("provider", &params, None).cwd,
            r"D:\Developer\codeterm"
        );
    }
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
