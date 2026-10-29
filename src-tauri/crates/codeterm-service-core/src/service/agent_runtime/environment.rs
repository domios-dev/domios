use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone)]
struct LaunchEnvironment {
    provider: String,
    nonce: String,
    extra: HashMap<String, String>,
}
static ENVIRONMENTS: LazyLock<Mutex<HashMap<String, LaunchEnvironment>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn revoke(tab: &str) {
    ENVIRONMENTS.lock().remove(tab);
}

pub(super) fn compose(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
) -> HashMap<String, String> {
    let extra = ENVIRONMENTS
        .lock()
        .get(tab)
        .filter(|env| env.provider == provider && env.nonce == nonce)
        .map(|env| env.extra.clone())
        .unwrap_or_default();
    compose_owned_environment(extra, svc.tab_env(tab))
}

fn compose_owned_environment(
    mut extra: HashMap<String, String>,
    owned: HashMap<String, String>,
) -> HashMap<String, String> {
    for key in [
        "CODETERM_TAB_ID",
        "CODETERM_PLAN_ID",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
        "CODETERM_ROLE",
        "CODETERM_TEAM_ID",
        "CODETERM_PARENT_TAB_ID",
        "CODETERM_WORKER_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_SHELL_NONCE",
        "CODETERM_API_URL",
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_SOCKET_PATH",
        "CODETERM_PAD_DIR",
        "CODETERM_LANE_BASE",
    ] {
        extra.remove(key);
    }
    extra.extend(owned);
    extra
}

impl ServiceContext {
    pub(crate) fn prepare_agent_runtime_launch_env(
        &self,
        tab: &str,
        extra_env: &HashMap<String, String>,
    ) -> Result<(), String> {
        if extra_env.len() > 256
            || extra_env
                .iter()
                .any(|(key, value)| key.contains(['=', '\0']) || value.contains('\0'))
            || extra_env
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
                > 4 * 1024 * 1024
        {
            return Err("Runtime launch environment exceeds host bounds".into());
        }
        let (provider, nonce) = self
            .mux
            .read()
            .get_tab(tab)
            .map(|target| {
                (
                    target.agent_type.clone(),
                    target.session_binding_nonce.clone(),
                )
            })
            .ok_or("Runtime tab unavailable")?;
        let provider = provider.ok_or("Runtime provider unavailable")?;
        let nonce = nonce.ok_or("Runtime binding nonce unavailable")?;
        if !super::engine::requested(self, &provider) {
            return Err("Runtime capability unavailable".into());
        }
        ENVIRONMENTS.lock().insert(
            tab.into(),
            LaunchEnvironment {
                provider,
                nonce,
                extra: extra_env.clone(),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
