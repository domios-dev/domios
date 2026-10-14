use codeterm_service_core::service::{plan_authority_fixture as fixture, AuthenticatedPlanCaller};
use codeterm_types::pty::BootStage;

#[tokio::test(flavor = "current_thread")]
async fn http_plan_authority_wait_does_not_block_another_request() {
    let (fixture, plan_id, _, owner) = fixture();
    let owner_tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&owner_tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.boot_stage = Some(BootStage::ProviderReady);
        tab.agent_set_at = Some(
            std::time::Instant::now()
                - codeterm_service_core::service::worker_ack::STARTUP_BINDING_DEADLINE
                + std::time::Duration::from_secs(3),
        );
    }
    let pending_service = std::sync::Arc::new(fixture.service.clone());
    let pending_plan = plan_id.clone();
    let pending = tokio::spawn(async move {
        crate::plan_routes::authorize_plan_orchestrator_http(&pending_service, &pending_plan, owner)
            .await
    });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());

    crate::plan_routes::authorize_plan_orchestrator_http(
        &std::sync::Arc::new(fixture.service.clone()),
        &plan_id,
        AuthenticatedPlanCaller::default(),
    )
    .await
    .expect("a concurrent user plan write can complete while the launch binding is pending");

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&owner_tab_id).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    codeterm_service_core::service::general_agent::binding_wait::notify_binding_state_changed();
    pending
        .await
        .expect("pending HTTP authorization task must finish")
        .expect("binding event must release the pending authorization");
}
