use axum::Json;

use super::SessionStartClaim;

fn authenticated_launch_identity(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> Option<crate::service::ProviderSessionBinding> {
    if !claim.nonce_verified {
        return None;
    }
    let binding_nonce = report_nonce?;
    let mux = mux.read();
    mux.get_tab(tab_id).and_then(|tab| {
        (tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce))
            .then(|| tab.pty_id.clone())
            .flatten()
            .map(|pty_id| crate::service::ProviderSessionBinding {
                pty_id,
                binding_nonce: binding_nonce.to_string(),
            })
    })
}

pub(super) fn acknowledge_authenticated_launch(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> bool {
    authenticated_launch_identity(mux, tab_id, claim, report_nonce).is_some_and(|binding| {
        let launch_acknowledged = crate::service::record_provider_session(tab_id, &binding);
        let readiness_acknowledged = crate::service::launch_readiness::note_acknowledged(
            tab_id,
            &binding.pty_id,
            &binding.binding_nonce,
        );
        launch_acknowledged || readiness_acknowledged
    })
}

fn acknowledge_and_reconcile_session_binding(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> Option<crate::service::ProviderSessionBinding> {
    let launch_identity = authenticated_launch_identity(&svc.mux, tab_id, claim, report_nonce);
    acknowledge_authenticated_launch(&svc.mux, tab_id, claim, report_nonce);
    if claim.nonce_verified {
        svc.rearm_authenticated_agent_input(tab_id);
        svc.reconcile_boot_stage_with_session_binding(tab_id);
    }
    launch_identity
}

pub(super) fn unchanged_acknowledged_hook_report(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    source: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> Json<serde_json::Value> {
    log::info!(
        "[hook-duplicate] tab={} provider={} source={} session={} nonce_verified={} nonce_already_acknowledged={}",
        super::escaped_hook_log_field(tab_id),
        super::escaped_hook_log_field(provider_id),
        super::escaped_hook_log_field(source),
        super::escaped_hook_log_field(session_id),
        claim.nonce_verified,
        claim.nonce_already_acknowledged,
    );
    acknowledge_and_reconcile_session_binding(svc, tab_id, claim, report_nonce);
    Json(serde_json::json!({"ok": true, "changed": false}))
}
