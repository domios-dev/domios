//! The single launch-env stamper. Every PTY launch path composes here, so one
//! tab record yields one identity map no matter which path built the PTY.
use crate::multiplexer::types::Tab;
use crate::service::instance_env::InstanceEnv;
use crate::service::plan::PlanStore;
use crate::service::ServiceContext;
use std::collections::HashMap;

/// Which launch surface is composing. Identity is identical for both surfaces;
/// the only permitted difference is `frontend_only_env()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchSurface {
    /// Daemon-side activation: agent launch, send-activation, restore sweep.
    Service,
    /// `spawn_pty` Tauri command and its web mirror `POST /api/pty/spawn`.
    Frontend,
}

/// Non-identity vars only the frontend surface has ever stamped. Deliberate and
/// asserted, so a new divergence cannot arrive by accident; identity never
/// belongs here.
pub(crate) fn frontend_only_env() -> [(&'static str, &'static str); 1] {
    [("CODETERM_VERSION", codeterm_build_info::version())]
}

/// Everything a launch stamps because a tab record exists. A PTY with no tab
/// record yet composes without it and therefore carries no identity at all.
pub(crate) struct TabLaunchEnv {
    pub tab_id: String,
    pub pad_dir: Option<String>,
    pub session_binding_nonce: String,
    pub shell_nonce: String,
    pub api_token: Option<String>,
    pub identity: HashMap<String, String>,
    pub task: HashMap<String, String>,
}

pub(crate) fn compose_launch_env(
    instance: &InstanceEnv,
    tab: Option<&TabLaunchEnv>,
    surface: LaunchSurface,
) -> HashMap<String, String> {
    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("CODETERM_SOCKET_PATH".into(), instance.socket_path.clone());
    env_vars.insert("CODETERM_API_URL".into(), instance.api_url.clone());
    env_vars.insert("CODETERM_API_PORT".into(), instance.api_port.clone());
    if let Some(path) = &instance.cli_path {
        env_vars.insert(
            "CODETERM_CLI_PATH".into(),
            path.to_string_lossy().into_owned(),
        );
    }
    if matches!(surface, LaunchSurface::Frontend) {
        env_vars.extend(
            frontend_only_env()
                .into_iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
    }
    if let Some(tab) = tab {
        if let Some(token) = &tab.api_token {
            env_vars.insert("CODETERM_API_TOKEN".into(), token.clone());
        }
        env_vars.insert("CODETERM_TAB_ID".into(), tab.tab_id.clone());
        if let Some(ref pad_dir) = tab.pad_dir {
            env_vars.insert("CODETERM_PAD_DIR".into(), pad_dir.clone());
        }
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            tab.session_binding_nonce.clone(),
        );
        env_vars.insert("CODETERM_SHELL_NONCE".into(), tab.shell_nonce.clone());
        env_vars.extend(tab.identity.clone());
        env_vars.extend(tab.task.clone());
    }
    // The CLI's env checks test for non-empty, so a blank var is worse than an
    // absent one — absence is the honest encoding of "the record has none".
    env_vars.retain(|_, value| !value.is_empty());
    env_vars
}

fn compose_task_env(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempts: &[crate::plan::TaskExecutionAttempt],
    lane_base: Option<&str>,
) -> HashMap<String, String> {
    let attempt = attempts.iter().rev().find(|attempt| {
        attempt.executor_tab_id == tab_id
            && attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
    });
    let mut env = HashMap::from([
        ("CODETERM_PLAN_ID".into(), plan_id.into()),
        ("CODETERM_TASK_ID".into(), task_id.into()),
    ]);
    if let Some(attempt) = attempt {
        env.insert("CODETERM_ATTEMPT_ID".into(), attempt.id.clone());
    }
    if let Some(base) = attempt
        .and_then(|attempt| attempt.base.as_deref())
        .or(lane_base)
        .filter(|base| !base.trim().is_empty())
    {
        env.insert("CODETERM_LANE_BASE".into(), base.into());
    }
    env
}

#[cfg(test)]
pub(crate) fn active_tab_lane_base(plan_store: &PlanStore, tab_id: &str) -> Option<String> {
    tab_task_env(plan_store, tab_id).remove("CODETERM_LANE_BASE")
}

pub(crate) fn tab_task_env(plan_store: &PlanStore, tab_id: &str) -> HashMap<String, String> {
    if let Some((plan_id, task_id)) = plan_store.task_binding_for_tab(tab_id) {
        return plan_store
            .get(&plan_id)
            .and_then(|plan| {
                plan.tasks
                    .iter()
                    .find(|task| {
                        task.id == task_id || task.handle.as_deref() == Some(task_id.as_str())
                    })
                    .map(|task| {
                        compose_task_env(
                            &plan_id,
                            &task_id,
                            tab_id,
                            &task.execution_attempts,
                            task.lane.base.as_deref(),
                        )
                    })
            })
            .unwrap_or_default();
    }
    if let Some(plan_id) = plan_store.planner_plan_id_for_tab(tab_id) {
        return HashMap::from([("CODETERM_PLAN_ID".into(), plan_id)]);
    }
    HashMap::new()
}

fn task_binding_env(tab: &Tab) -> HashMap<String, String> {
    let Some(binding) = tab.task_binding.as_ref() else {
        return HashMap::new();
    };
    [
        ("CODETERM_PLAN_ID", binding.plan_id.as_ref()),
        ("CODETERM_TASK_ID", binding.task_id.as_ref()),
        ("CODETERM_ATTEMPT_ID", binding.attempt_id.as_ref()),
    ]
    .into_iter()
    .filter_map(|(key, value)| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| (key.into(), value.to_string()))
    })
    .collect()
}

pub(crate) fn tab_identity_env(tab: &Tab) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    if tab.agent_type.is_some() {
        env_vars.insert(
            crate::pty::shell_integration::AGENT_SHELL_ENV.into(),
            "1".into(),
        );
    }
    if let Some(ref team_id) = tab.team_id {
        env_vars.insert("CODETERM_TEAM_ID".into(), team_id.clone());
    }
    if let Some(ref role) = tab.role {
        env_vars.insert("CODETERM_ROLE".into(), format!("{:?}", role));
    }
    if let Some(ref parent) = tab.parent_tab_id {
        env_vars.insert("CODETERM_PARENT_TAB_ID".into(), parent.clone());
    }
    if let Some(ref worker_id) = tab.worker_id {
        env_vars.insert("CODETERM_WORKER_ID".into(), worker_id.clone());
    }
    env_vars
}

impl ServiceContext {
    /// Pad and nonces are resolved before the mux read so no lock is held across
    /// a record write (lock order: mux, then the rest).
    fn tab_launch_env(&self, tab_id: &str) -> TabLaunchEnv {
        let pad_dir = self
            .ensure_agent_pad(tab_id)
            .ok()
            .map(|pad| pad.env_value());
        let session_binding_nonce = self.ensure_session_binding_nonce(tab_id);
        let shell_nonce = self.ensure_shell_integration_nonce(tab_id);
        let (mut identity, mut task, utility_shell, team_shell) = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .map(|tab| {
                    let task = if tab.is_utility_shell() {
                        HashMap::new()
                    } else {
                        task_binding_env(tab)
                    };
                    (
                        tab_identity_env(tab),
                        task,
                        tab.is_utility_shell(),
                        tab.is_utility_shell()
                            && tab.team_id.is_some()
                            && !tab
                                .shell_profile
                                .as_ref()
                                .is_some_and(|p| p.wsl_distro.is_some()),
                    )
                })
                .unwrap_or_default()
        };
        if !utility_shell {
            task.extend(tab_task_env(&self.plan_store, tab_id));
        }
        if team_shell {
            super::super::spawn_shell::apply_team_shell_path(&mut identity);
        }
        let api_token = self.mint_tab_credential(tab_id, &session_binding_nonce);
        TabLaunchEnv {
            tab_id: tab_id.to_string(),
            pad_dir,
            session_binding_nonce,
            shell_nonce,
            api_token,
            identity,
            task,
        }
    }

    pub(crate) fn tab_env(&self, tab_id: &str) -> HashMap<String, String> {
        compose_launch_env(
            &InstanceEnv::from_api_config(self.configuration().api),
            Some(&self.tab_launch_env(tab_id)),
            LaunchSurface::Service,
        )
    }

    /// Frontend/REST spawn, including the post-update auto-resume path. A
    /// `None` tab id is a PTY with no tab record yet: instance vars only.
    pub fn frontend_pty_env(&self, tab_id: Option<&str>) -> HashMap<String, String> {
        let tab = tab_id.map(|tab_id| self.tab_launch_env(tab_id));
        compose_launch_env(
            &InstanceEnv::from_api_config(self.configuration().api),
            tab.as_ref(),
            LaunchSurface::Frontend,
        )
    }
}

#[cfg(test)]
mod tests;
