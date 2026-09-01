//! The single launch-env stamper. Every PTY launch path composes here, so one
//! tab record yields one identity map no matter which path built the PTY.
use crate::multiplexer::types::Tab;
use crate::service::plan::PlanStore;
use crate::service::ServiceContext;
use std::collections::HashMap;

/// Which launch surface is composing. Identity is identical for both surfaces;
/// the only permitted difference is `FRONTEND_ONLY_ENV`.
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
pub(crate) const FRONTEND_ONLY_ENV: &[(&str, &str)] =
    &[("CODETERM_VERSION", env!("CARGO_PKG_VERSION"))];

/// Instance-wide launch vars — same for every tab of this daemon.
pub(crate) struct InstanceEnv {
    pub socket_path: String,
    pub api_url: String,
    pub api_port: String,
    pub api_token: Option<String>,
}

/// Everything a launch stamps because a tab record exists. A PTY with no tab
/// record yet composes without it and therefore carries no identity at all.
pub(crate) struct TabLaunchEnv {
    pub tab_id: String,
    pub pad_dir: Option<String>,
    pub session_binding_nonce: String,
    pub shell_nonce: String,
    pub identity: HashMap<String, String>,
    pub task: HashMap<String, String>,
}

impl InstanceEnv {
    pub(crate) fn resolve() -> Self {
        let api = crate::config::load_config().api;
        let (api_url, api_port) = crate::config::api_callback_url(&api);
        Self {
            socket_path: crate::socket::server::socket_display_path(),
            api_url,
            api_port,
            api_token: api.token,
        }
    }
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
    if let Some(ref token) = instance.api_token {
        env_vars.insert("CODETERM_API_TOKEN".into(), token.clone());
    }
    if matches!(surface, LaunchSurface::Frontend) {
        env_vars.extend(
            FRONTEND_ONLY_ENV
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
    }
    if let Some(tab) = tab {
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

pub(crate) fn active_tab_lane_base(plan_store: &PlanStore, tab_id: &str) -> Option<String> {
    let (plan_id, task_id) = plan_store.task_binding_for_tab(tab_id)?;
    plan_store
        .get(&plan_id)
        .and_then(|plan| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id.as_str()))
                .and_then(|task| {
                    task.execution_attempts
                        .iter()
                        .rev()
                        .find(|attempt| {
                            attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                        })
                        .and_then(|attempt| attempt.base.clone())
                        .or_else(|| task.lane.base.clone())
                })
        })
        .filter(|base| !base.trim().is_empty())
}

pub(crate) fn tab_task_env(plan_store: &PlanStore, tab_id: &str) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    if let Some((plan_id, task_id)) = plan_store.task_binding_for_tab(tab_id) {
        if let Some(base) = active_tab_lane_base(plan_store, tab_id) {
            env_vars.insert("CODETERM_LANE_BASE".into(), base);
        }
        env_vars.insert("CODETERM_PLAN_ID".into(), plan_id);
        env_vars.insert("CODETERM_TASK_ID".into(), task_id);
    } else if let Some(plan_id) = plan_store.planner_plan_id_for_tab(tab_id) {
        // A Planner binds to its plan directly, with no task — codeterm#371.
        env_vars.insert("CODETERM_PLAN_ID".into(), plan_id);
    }
    env_vars
}

pub(crate) fn tab_identity_env(tab: &Tab) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
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
        let identity = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .map(tab_identity_env)
                .unwrap_or_default()
        };
        TabLaunchEnv {
            tab_id: tab_id.to_string(),
            pad_dir,
            session_binding_nonce,
            shell_nonce,
            identity,
            task: tab_task_env(&self.plan_store, tab_id),
        }
    }

    pub(crate) fn tab_env(&self, tab_id: &str) -> HashMap<String, String> {
        compose_launch_env(
            &InstanceEnv::resolve(),
            Some(&self.tab_launch_env(tab_id)),
            LaunchSurface::Service,
        )
    }

    /// Frontend/REST spawn, including the post-update auto-resume path. A
    /// `None` tab id is a PTY with no tab record yet: instance vars only.
    pub(crate) fn frontend_pty_env(&self, tab_id: Option<&str>) -> HashMap<String, String> {
        let tab = tab_id.map(|tab_id| self.tab_launch_env(tab_id));
        compose_launch_env(
            &InstanceEnv::resolve(),
            tab.as_ref(),
            LaunchSurface::Frontend,
        )
    }
}

#[cfg(test)]
mod tests;
