use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TargetRoute {
    Pty,
    Runtime,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HandoffRoute {
    pub source: TargetRoute,
    pub target: TargetRoute,
}

pub(super) fn route(
    source: (ProviderTransport, bool),
    target: (ProviderTransport, bool),
    same_provider: bool,
    backing: &TabBacking,
) -> Result<HandoffRoute, String> {
    if same_provider {
        return Err("provider_switch_same_provider: select a different provider".into());
    }
    let classify = |(transport, runtime)| {
        if runtime {
            Ok(TargetRoute::Runtime)
        } else if transport == ProviderTransport::Pty {
            Ok(TargetRoute::Pty)
        } else {
            Err("provider_switch_capability_mismatch: provider lacks managed agent lifecycle and binding support".to_owned())
        }
    };
    let source = classify(source)?;
    let target = classify(target)?;
    if !matches!(
        (source, backing),
        (TargetRoute::Pty, TabBacking::Pty)
            | (TargetRoute::Runtime, TabBacking::ChatBackend { .. })
    ) {
        return Err(
            "provider_switch_backing_mismatch: source transport does not match its backing".into(),
        );
    }
    Ok(HandoffRoute { source, target })
}

pub(super) fn require_idle(idle: bool, force: bool, booting: bool) -> Result<(), String> {
    if booting {
        return Err("provider_switch_launch_busy: wait for the current launch to finish".into());
    }
    if !idle && !force {
        return Err("provider_switch_busy: wait for idle or stop the current turn first".into());
    }
    Ok(())
}

pub(super) fn reset_for_target(
    tab: &mut Tab,
    target: &str,
    route: TargetRoute,
    launch: &RestorableLaunchParameters,
) {
    provider_handoff::reset_tab_identity_for_provider_change(
        tab,
        launch.permission_bypass_requested,
    );
    tab.restorable_launch = launch.clone();
    tab.agent_type = Some(target.into());
    tab.agent_session_id = None;
    tab.agent_session_name = None;
    tab.agent_state = None;
    tab.session_binding_nonce = None;
    tab.reported_session_binding_nonce = None;
    tab.restored_from_snapshot = false;
    tab.close_restore = None;
    tab.pty_id = None;
    tab.agent_booting = true;
    tab.agent_runtime_state = None;
    tab.agent_runtime_config = None;
    tab.fork_source_session_id = None;
    tab.agent_requested_model = launch.requested_model.clone();
    tab.agent_provider_resolved_model = launch.model.clone();
    tab.reasoning_effort_identity.requested = launch.requested_reasoning_effort.clone();
    tab.reasoning_effort_identity.resolved = launch.resolved_reasoning_effort.clone();
    tab.provider_signals = Default::default();
    tab.backing = match route {
        TargetRoute::Pty => TabBacking::Pty,
        TargetRoute::Runtime => TabBacking::ChatBackend {
            plugin_id: target.into(),
            session_id: String::new(),
        },
    };
    if route == TargetRoute::Runtime {
        tab.chat_view = true;
        tab.agent_runtime_config = Some(
            serde_json::json!({"model": launch.model, "effort": launch.resolved_reasoning_effort}),
        );
    }
}

#[cfg(test)]
mod tests;
