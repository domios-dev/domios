use crate::service::{ServiceContext, SpawnAgentParams};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static FALLBACKS: LazyLock<Mutex<HashMap<String, (String, Option<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn requires_fresh_pty(tab: &str) -> bool {
    FALLBACKS.lock().contains_key(tab)
}
pub fn completed(tab: &str) {
    FALLBACKS.lock().remove(tab);
}

pub fn recovering(tab: &str, nonce: &str) -> bool {
    FALLBACKS
        .lock()
        .get(tab)
        .is_some_and(|(generation, _)| generation == nonce)
}

pub fn after_relink(svc: &ServiceContext, tab: &str, pty: &str) {
    let replaced = FALLBACKS
        .lock()
        .get(tab)
        .is_some_and(|(_, previous)| previous.as_deref() != Some(pty));
    if replaced {
        super::lifecycle::clear_tab(tab);
        if let Some(tab) = svc.mux.write().get_tab_mut(tab) {
            tab.structured_control_active = false;
        }
    }
}

fn should_recover(
    current: bool,
    structured: bool,
    pending: bool,
    already_recovering: bool,
) -> bool {
    current && structured && pending && !already_recovering
}

pub fn try_start(svc: &ServiceContext, tab_id: &str, nonce: &str, reason: &str) -> bool {
    let sidecar = super::lifecycle::for_tab(tab_id);
    let tab = svc.mux.read().get_tab(tab_id).cloned();
    let current = tab
        .as_ref()
        .is_some_and(|tab| tab.session_binding_nonce.as_deref() == Some(nonce));
    let pending =
        super::lifecycle::delivery_state(tab_id) == super::lifecycle::DeliveryState::Pending;
    {
        let mut fallbacks = FALLBACKS.lock();
        if !should_recover(
            current,
            sidecar.is_some(),
            pending,
            fallbacks.contains_key(tab_id),
        ) {
            return false;
        }
        fallbacks.insert(
            tab_id.into(),
            (
                nonce.into(),
                tab.as_ref().and_then(|tab| tab.pty_id.clone()),
            ),
        );
    }
    let sidecar = sidecar.expect("structured recovery has an endpoint");
    let tab = tab.expect("current recovery has a tab");
    let Some(provider) = tab.agent_type.clone() else {
        return false;
    };
    let thread = sidecar.thread.lock().clone();
    let authorization = crate::service::permission_policy::authorize_permission_bypass(
        &svc.agent_registry,
        &provider,
        sidecar.spawn.full_access,
    );
    let (model, effort) = {
        let controls = sidecar.controls.lock();
        (controls.model.clone(), controls.effort.clone())
    };
    let params = SpawnAgentParams {
        agent: Some(provider.clone()),
        cwd: tab.cwd.clone(),
        resume_session_id: Some(thread.clone()),
        model,
        reasoning_effort: effort,
        skip_permissions: Some(authorization.is_some()),
        permission_bypass_authorization: authorization,
        context_files: tab.restorable_launch.context_files.clone(),
        context_skills: tab.restorable_launch.context_skills.clone(),
        args: tab.restorable_launch.args.clone(),
        agent_name: tab.restorable_launch.agent_name.clone(),
        starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
        ..Default::default()
    };
    let command = crate::service::orchestrator::build_agent_command(
        &provider,
        &params,
        sidecar.spawn.instructions.as_deref(),
        None,
        &svc.agent_registry,
    );
    log::warn!("[codex-rpc-attach] tab={tab_id} nonce={nonce} thread={thread} recovery=plain_resume reason={reason}");
    if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = true;
    }
    crate::service::orchestrator::launch_agent_in_background_with_env(
        svc.clone(),
        tab_id.into(),
        command,
        provider,
        crate::service::launch_diagnostics::AgentLaunchOrigin::AutomaticExitRecovery,
        false,
        None,
        None,
        None,
        Default::default(),
        None,
        None,
        Some(thread),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_attach_failure_recovers_current_pending_launch_once() {
        assert!(should_recover(true, true, true, false));
        assert!(!should_recover(false, true, true, false));
        assert!(!should_recover(true, false, true, false));
        assert!(!should_recover(true, true, false, false));
        assert!(!should_recover(true, true, true, true));
    }
}
