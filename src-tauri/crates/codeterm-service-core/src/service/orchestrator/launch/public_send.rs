use crate::service::public_send_readiness::{PublicSendGateKey, PublicSendLaunchGuard};
use crate::service::ServiceContext;

pub(super) fn arm(
    svc: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    expected_turn: Option<String>,
) -> Option<PublicSendGateKey> {
    let had_session = svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.agent_session_id.is_some());
    let anchor = expected_turn
        .as_ref()
        .and_then(|_| svc.chat_send_anchor(tab_id).anchor);
    let expected_turn = if had_session && anchor.is_none() {
        None
    } else {
        expected_turn
    };
    let wait_for_turn_completion = svc
        .agent_registry
        .with_provider(agent_cmd, |provider| {
            provider.public_send_waits_for_startup_turn_completion()
        })
        .unwrap_or(false);
    svc.arm_public_send_launch(tab_id, expected_turn, anchor, wait_for_turn_completion)
}

pub(super) fn guard(
    svc: &ServiceContext,
    tab_id: &str,
    key: Option<PublicSendGateKey>,
) -> PublicSendLaunchGuard {
    svc.public_send_launch_guard_for_key(tab_id, key)
}
