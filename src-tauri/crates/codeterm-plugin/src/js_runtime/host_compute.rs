//! `host.worker` / `host.agent` / `host.workspace` / `host.mem` handles.
//!
//! All service calls delegate through `PluginHostCtx`; no app types leak in.

use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use rquickjs::function::Func;
use rquickjs::Object;

use super::host_api::PluginSandbox;
use super::vm::VmError;
use crate::activity::{PluginActivity, PluginActivityRegistry};

const TURN_WATCHDOG_SECS: u64 = 30 * 60;

static PROGRESS_ACTIVITY_IDS: LazyLock<Mutex<std::collections::HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn err(msg: impl Into<String>) -> String {
    serde_json::json!({ "error": msg.into() }).to_string()
}

#[derive(serde::Deserialize, Default)]
pub(super) struct Backend {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(serde::Deserialize)]
struct ProgressOpts {
    label: String,
    #[serde(default)]
    percent: Option<f32>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

pub(super) fn backend_provider_model(backend: &Option<Backend>) -> (String, String) {
    match backend {
        Some(b) => (
            b.provider.clone().unwrap_or_else(|| "claude".to_string()),
            b.model.clone().unwrap_or_default(),
        ),
        None => ("claude".to_string(), String::new()),
    }
}

pub(super) fn apply_progress_raw(
    registry: &PluginActivityRegistry,
    current_id: &mut Option<String>,
    plugin_id: &str,
    opts_json: &str,
) -> Result<PluginActivity, String> {
    let opts: ProgressOpts =
        serde_json::from_str(opts_json).map_err(|e| format!("bad progress opts: {e}"))?;
    if opts.label.trim().is_empty() {
        return Err("progress label is required".to_string());
    }

    let mut activity = match current_id
        .as_ref()
        .filter(|id| registry.get(id).is_some())
        .cloned()
    {
        Some(id) => registry.step(&id, &opts.label, opts.percent),
        None => {
            let started = registry.start(plugin_id, "bootstrap", &opts.label);
            *current_id = Some(started.id.clone());
            if opts.percent.is_some() {
                registry.step(&started.id, &opts.label, opts.percent)
            } else {
                started
            }
        }
    };

    if opts.done || opts.error.is_some() {
        activity = registry.finish(&activity.id, opts.error);
        *current_id = None;
    }

    Ok(activity)
}

pub(super) fn agent_resume_json(session_id: &str) -> String {
    serde_json::json!({ "sessionId": session_id }).to_string()
}

pub(super) fn agent_get_json() -> String {
    "null".to_string()
}

pub(super) fn agent_usage_json() -> String {
    "null".to_string()
}

pub(super) fn install_handles(
    host: &Object<'_>,
    sandbox: &Arc<PluginSandbox>,
) -> Result<(), VmError> {
    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    let sb = sandbox.clone();
    bind!("workerStart", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => svc.worker_start_json(sb.plugin_id(), &opts),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("workerPoll", move |job_id: String| -> String {
        match sb.service() {
            Some(svc) => svc.worker_poll_json(&job_id),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("progressRaw", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => svc.progress_raw_json(sb.plugin_id(), &opts),
            None => "{}".to_string(),
        }
    });

    let sb = sandbox.clone();
    bind!("workspaceEnsure", move |opts: String| -> String {
        if !sb.manage_workspaces_allowed() {
            return err("host.workspace: manage_workspaces permission required");
        }
        match sb.service() {
            Some(svc) => svc.workspace_ensure_json(&opts),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("workspacePanes", move |workspace_id: String| -> String {
        if !sb.manage_workspaces_allowed() {
            return err("host.workspace: manage_workspaces permission required");
        }
        match sb.service() {
            Some(svc) => svc.workspace_panes_json(&workspace_id),
            None => err("host service unavailable"),
        }
    });

    const AGENT_DENIED: &str = "host.agent: spawn_agents permission required";

    let sb = sandbox.clone();
    bind!("agentSpawn", move |workspace_id: String,
                              opts: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => svc.agent_spawn_json(sb.plugin_id(), &workspace_id, &opts),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentResume", move |_workspace_id: String,
                               session_id: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_resume_json(&session_id),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentGet", move |_workspace_id: String,
                            _key: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_get_json(),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentSend", move |session_id: String,
                             text: String|
          -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => svc.agent_send_json(sb.plugin_id(), &session_id, &text),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentPoll", move |ticket: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => svc.agent_poll_json(&ticket),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentReap", move |session_id: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(svc) => svc.agent_reap_json(&session_id),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("agentUsage", move |_session_id: String| -> String {
        if !sb.spawn_agents_allowed() {
            return err(AGENT_DENIED);
        }
        match sb.service() {
            Some(_) => agent_usage_json(),
            None => err("host service unavailable"),
        }
    });

    let sb = sandbox.clone();
    bind!("memSearch", move |opts: String| -> String {
        match sb.service() {
            Some(svc) => svc.mem_search_json(&opts),
            None => err("host service unavailable"),
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests;
