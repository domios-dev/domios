use crate::dispatch::CommandPort;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{PendingPrompt, PromptOption};
use codeterm_types::pty::{BootStage, TabStatus, TabVerdict, TimelineState};

fn summary_status(service: &crate::service::ServiceContext, tab_id: &str) -> String {
    let snapshot = service.mux_snapshot();
    crate::dispatch::build_tab_summary_json(&snapshot, None, None, false, false)
        .into_iter()
        .find(|row| row["id"] == tab_id)
        .and_then(|row| row["status"].as_str().map(str::to_string))
        .expect("summary row")
}

#[test]
fn compact_status_and_summary_conserve_boot_failure_and_missing_binding() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.role = Some(crate::multiplexer::types::AgentRole::Orchestrator);
    tab.agent_booting = true;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    for (stage, booting, expected) in [
        (Some(BootStage::PtyStarting), true, "starting"),
        (Some(BootStage::LaunchBindingUnconfirmed), false, "unknown"),
        (Some(BootStage::SpawnFailed), false, "error"),
        (Some(BootStage::ShellReady), false, "unknown"),
        (Some(BootStage::AgentReady), false, "unknown"),
        (None, false, "unknown"),
    ] {
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.boot_stage = stage;
            tab.agent_booting = booting;
        }
        assert_eq!(
            fixture.service.get_tab_status(&id).unwrap().status,
            expected
        );
        assert_eq!(summary_status(&fixture.service, &id), expected);
    }
}

#[test]
fn tab_status_single_source() {
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-single-source".into());
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = Some("current-session".into());
        tab.session_binding_nonce = Some("current-nonce".into());
        tab.reported_session_binding_nonce = Some("current-nonce".into());
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.agent_state = Some(
            TabVerdict {
                state: TimelineState::Idle,
                subtype: None,
            }
            .into(),
        );
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let agree = |expected: TabStatus| {
        let status = service.get_tab_status(&tab_id).unwrap().status;
        assert_eq!(status, expected.label());
        assert_eq!(summary_status(service, &tab_id), status);
    };

    service
        .agent_tracker
        .lock()
        .update_output("pty-single-source", [3u8; 32]);
    agree(TabStatus::Working);

    service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pending_prompt = Some(PendingPrompt {
        request_id: None,
        question: "opaque-question-token".into(),
        options: vec![PromptOption {
            label: "opaque-option".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    });
    agree(TabStatus::Waiting);

    service.clear_pending_prompt(&tab_id);
    service.agent_tracker.lock().set_idle("pty-single-source");
    agree(TabStatus::Idle);
    service.agent_tracker.lock().set_error("pty-single-source");
    agree(TabStatus::Error);

    service.agent_tracker.lock().set_dead("pty-single-source");
    agree(TabStatus::Dead);
}

#[test]
fn provider_turn_error_surfaces_status() {
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-provider-error".into());
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = Some("current-session".into());
        tab.session_binding_nonce = Some("current-nonce".into());
        tab.reported_session_binding_nonce = Some("current-nonce".into());
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    service
        .agent_tracker
        .lock()
        .update_output("pty-provider-error", [7u8; 32]);

    assert!(service.mark_agent_turn_error(&tab_id));
    assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "error");
    assert_eq!(summary_status(service, &tab_id), "error");
}

#[test]
fn provider_turn_error_after_user_interrupt_surfaces_idle_status() {
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-interrupted-provider-error".into());
        tab.agent_type = Some("opaque-provider".into());
        tab.agent_session_id = Some("current-session".into());
        tab.session_binding_nonce = Some("current-nonce".into());
        tab.reported_session_binding_nonce = Some("current-nonce".into());
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    service
        .agent_tracker
        .lock()
        .update_output("pty-interrupted-provider-error", [8u8; 32]);
    service.arm_agent_stop_suppression("pty-interrupted-provider-error");

    assert!(service.mark_agent_turn_error(&tab_id));
    assert_eq!(service.get_tab_status(&tab_id).unwrap().status, "idle");
    assert_eq!(summary_status(service, &tab_id), "idle");
}

#[test]
fn get_tab_status_projects_restore_failure_state_and_one_line_reason() {
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque-provider".into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("\n opaque-boot-reason \nopaque-detail".into());
        tab.agent_runtime_state = Some(serde_json::json!({
            "state": "degraded", "epoch": 1, "failureCode": "session_load_refused",
            "reason": "opaque-runtime-reason"
        }));
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let json = serde_json::to_value(service.get_tab_status(&tab_id).unwrap()).unwrap();
    assert_eq!(
        json["failure"],
        serde_json::json!({"boot_stage": "spawn_failed", "runtime_state": "degraded",
            "code": "session_load_refused", "reason": "opaque-boot-reason"})
    );

    let runtime_only = super::tab_failure(
        Some(BootStage::AgentReady),
        Some("stale-boot-reason"),
        Some(
            &serde_json::json!({"state": "unloaded", "failureCode": "thread_not_found",
            "reason": "opaque-runtime-reason"}),
        ),
    )
    .unwrap();
    assert_eq!(runtime_only.boot_stage, None);
    assert_eq!(runtime_only.runtime_state.as_deref(), Some("unloaded"));
    assert_eq!(runtime_only.reason, "opaque-runtime-reason");

    {
        let mut mux = service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.boot_stage = Some(BootStage::AgentReady);
        tab.boot_failure_reason = None;
        tab.agent_runtime_state = Some(serde_json::json!({"state": "loaded", "epoch": 2}));
    }
    let healthy = serde_json::to_value(service.get_tab_status(&tab_id).unwrap()).unwrap();
    assert!(healthy.get("failure").is_none());
}

#[test]
fn get_tab_status_includes_pending_prompt_for_terminal_choice() {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().pending_prompt = Some(PendingPrompt {
        request_id: None,
        question: "opaque-question-token".into(),
        options: vec![
            PromptOption {
                label: "opaque-option-one".into(),
                value: "1".into(),
            },
            PromptOption {
                label: "opaque-option-two".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: Some(false),
        submit_available: Some(false),
        selected_values: None,
    });
    mux.get_tab_mut(&tab_id).unwrap().pending_prompt_kind =
        Some(codeterm_types::pty::PromptKind::Provider);
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    let status = fixture.service.get_tab_status(&tab_id).unwrap();

    assert_eq!(
        status.pending_prompt.as_ref().map(|prompt| {
            prompt
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>()
        }),
        Some(vec!["1", "2"])
    );
}
