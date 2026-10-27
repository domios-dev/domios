use super::dispatch_service_rpc;
use codeterm_service_core::cron::store::{CronCreateParams, CronRunInfo, CronRunStatus, CronStore};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

#[test]
fn caller_nonce_is_required_before_memory_or_agent_creation_dispatch() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    for method in ["mem.search", "agent.spawn", "tab.create"] {
        let error = dispatch_service_rpc(
            method,
            Some(serde_json::json!({"caller_tab_id":tab})),
            &Arc::new(fixture.service.clone()),
        )
        .unwrap_err();
        assert_eq!(error.code, -32003);
        assert_eq!(error.data.unwrap()["reason"], "neighbor_forbidden");
    }
}

#[test]
fn identify_uses_the_shared_product_name() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let result =
        dispatch_service_rpc("system.identify", None, &Arc::new(fixture.service.clone())).unwrap();
    assert_eq!(result["name"], "Domios");
    assert_eq!(result["name"], codeterm_build_info::PRODUCT_NAME);
    assert_eq!(
        result["name"],
        codeterm_service_core::dispatch::health_result()
            .unwrap()
            .into_json()["name"]
    );
}

#[test]
fn cron_settle_service_rpc_closes_completed_tab() {
    let (mut mux, initial_tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&initial_tab).unwrap();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let mut store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
    let job_id = store
        .create(CronCreateParams {
            name: "rpc completion".into(),
            workspace_id: "workspace".into(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "complete run".into(),
            description: None,
            agent: None,
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: Some(false),
            max_history: None,
            oneshot: Some(false),
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap()
        .id;
    store
        .record_run(
            &job_id,
            CronRunInfo {
                run_id: "service-rpc-complete-run".into(),
                started_at: 1,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some(tab_id.clone()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
    store.register_tab(&tab_id, &job_id, "service-rpc-complete-run");
    let cron: codeterm_service_core::CronState = Arc::new(parking_lot::Mutex::new(store));
    assert!(fixture.service.set_ext(cron).is_ok());
    assert!(fixture
        .service
        .set_ext::<codeterm_service_core::CronWake>(Arc::new(std::sync::Condvar::new()))
        .is_ok());

    let response = dispatch_service_rpc(
        "cron.complete",
        Some(serde_json::json!({
            "run_id": "service-rpc-complete-run",
            "summary": "service-rpc-summary",
            "human_confirmed": true
        })),
        &Arc::new(fixture.service.clone()),
    )
    .unwrap();

    assert_eq!(response["ok"], true);
    assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
}
