use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_types::pty::{BootStage, ProvisionalLaunch};

fn stuck_fixture() -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("late-hook-fixture".into());
    tab.agent_session_id = Some("sid-a".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.pty_id = Some("own-pty".into());
    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::LaunchDeadlineElapsed);
    tab.provisional_launch = Some(ProvisionalLaunch {
        pty_id: "own-pty".into(),
        binding_nonce: "own-nonce".into(),
        reason: "dropped SessionStart".into(),
        awaiting_binding_ack: false,
        first_turn_input_ready: false,
        first_turn_payload_written: false,
        first_turn_admitted_send_id: None,
    });
    let fixture = ServiceFixture::new(mux);
    tests::register_hook_providers(
        &fixture.service.agent_registry,
        vec![("late-hook-fixture", vec!["sid-a"])],
    );
    (fixture, tab_id)
}

#[tokio::test]
async fn stop_hook_recovers_dropped_session_start_before_returning() {
    let (fixture, tab_id) = stuck_fixture();
    let _response = hook_stop(
        AxumState(prompt_resolution_tests::app_state(&fixture)),
        Path(tab_id.clone()),
        HeaderMap::new(),
        OptionalJson(Some(serde_json::json!({
            "session_id": "sid-a", "binding_nonce": "own-nonce",
        }))),
    )
    .await
    .unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.provisional_launch.is_none());
    assert!(tab.launch_session_authenticated());
}

#[tokio::test]
async fn late_hook_refuses_unknown_and_foreign_provider_sessions_with_own_nonce() {
    for session in ["invented-session", "foreign-session"] {
        let (fixture, tab_id) = stuck_fixture();
        tests::register_hook_providers(
            &fixture.service.agent_registry,
            vec![("foreign-provider", vec!["foreign-session"])],
        );
        let result =
            codeterm_service_core::service::hook_session_binding::recover_verified_hook_binding(
                &fixture.service,
                &tab_id,
                None,
                Some(session),
                Some("own-nonce"),
            )
            .await;
        assert_eq!(result.unwrap_err().0, StatusCode::CONFLICT);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("sid-a"));
        assert_eq!(tab.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
        assert!(tab.reported_session_binding_nonce.is_none());
    }
}

#[tokio::test]
async fn late_hook_accepts_a_new_session_only_from_its_exact_provider_record() {
    let (fixture, tab_id) = stuck_fixture();
    tests::register_hook_providers(
        &fixture.service.agent_registry,
        vec![("late-hook-fixture", vec!["sid-a", "sid-b"])],
    );
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = None;
    codeterm_service_core::service::hook_session_binding::recover_verified_hook_binding(
        &fixture.service,
        &tab_id,
        None,
        Some("sid-b"),
        Some("own-nonce"),
    )
    .await
    .unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("sid-b"));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.launch_session_authenticated());
}

struct HookLogs(parking_lot::Mutex<Vec<String>>);
static HOOK_LOGS: HookLogs = HookLogs(parking_lot::Mutex::new(Vec::new()));

impl log::Log for HookLogs {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        let line = record.args().to_string();
        if line.contains("[hook-refusal]")
            || line.contains("[hook-deferred]")
            || line.contains("event=session-binding")
        {
            let mut lines = self.0.lock();
            if lines.len() < 100 {
                lines.push(line);
            }
        }
    }
    fn flush(&self) {}
}

#[tokio::test]
async fn session_start_refusals_claim_failures_and_deferrals_log_reasons() {
    log::set_logger(&HOOK_LOGS).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let (fixture, tab_id) = stuck_fixture();
    let state = prompt_resolution_tests::app_state(&fixture);
    let mut headers = HeaderMap::new();
    headers.insert("x-codeterm-hook", "owned-hook".parse().unwrap());
    assert!(hook_session_start(
        AxumState(state.clone()),
        Path(tab_id.clone()),
        headers,
        OptionalJson(Some(serde_json::json!({"session_id": "sid-a"}))),
    )
    .await
    .is_err());
    assert!(bind_reported_session(
        &fixture.service.mux,
        &fixture.service.agent_registry,
        &tab_id,
        "unknown-provider",
        "sid-a",
        Some("own-nonce"),
    )
    .is_err());
    defer_session_binding(
        state,
        &tab_id,
        "late-hook-fixture",
        "sid-b",
        "sid-a",
        fixture.temp.path().join("pending.jsonl"),
        Some("own-nonce".into()),
        "fixture".into(),
        serde_json::Value::Null,
    )
    .unwrap();
    assert!(
        codeterm_service_core::service::hook_session_binding::recover_verified_hook_binding(
            &fixture.service,
            &tab_id,
            None,
            Some("invented-session"),
            Some("own-nonce"),
        )
        .await
        .is_err()
    );
    codeterm_service_core::service::hook_session_binding::recover_verified_hook_binding(
        &fixture.service,
        &tab_id,
        None,
        Some("sid-a"),
        Some("own-nonce"),
    )
    .await
    .unwrap();
    let lines = HOOK_LOGS.0.lock();
    let own_lines: Vec<_> = lines.iter().filter(|line| line.contains(&tab_id)).collect();
    assert!(own_lines.iter().any(|line| line.contains("[hook-refusal]")
        && line.contains("event=session_start")
        && line.contains("missing its binding nonce")));
    assert!(own_lines.iter().any(|line| line.contains("[hook-refusal]")
        && line.contains("event=session_claim")
        && line.contains("unknown provider_id")));
    assert!(own_lines.iter().any(|line| line.contains("[hook-deferred]")
        && line.contains("reason=transcript_not_materialized")));
    assert!(own_lines.iter().any(|line| line.contains("[hook-refusal]")
        && line.contains("reason=session_not_owned_by_launch_provider")));
    let timeline = own_lines
        .iter()
        .find(|line| line.contains("event=session-binding"))
        .unwrap();
    assert!(timeline.contains("report_nonce_present=true"));
    assert!(timeline.contains("current_nonce_present=true"));
    assert!(timeline.contains("reported_nonce_present=true"));
    assert!(timeline.contains("report_matches_current=true"));
    assert!(timeline.contains("reported_matches_current=true"));
    assert!(!timeline.contains("own-nonce"));
}
