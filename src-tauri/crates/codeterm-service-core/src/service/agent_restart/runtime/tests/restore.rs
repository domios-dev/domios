use super::*;
use crate::agent::chat_types::ChatMsg;

fn refused_load_fixture(saved: &[ChatMsg]) -> (RuntimeFixture, String, Value) {
    let methods = [
        "initialize",
        "openThread",
        "resumeThread",
        "submitTurn",
        "control",
    ]
    .into_iter()
    .map(|kind| (kind.to_owned(), json!([kind])))
    .collect::<serde_json::Map<String, Value>>();
    let source = SOURCE.replace("code:'provider_refused'", "code:'session_load_refused'");
    let fixture = RuntimeFixture::prepared_with_runtime(
        &source,
        true,
        json!({"requestMethods":methods,"launchProfiles":{"opaque-profile":{"binary":"node","args":[]}}}),
    );
    let old_thread = uuid::Uuid::new_v4().to_string();
    let config = json!({"systemPrompt":format!("{PROMPT} opaque refused identity"),"model":"supplied-model","effort":"opaque-effort","launchProfile":"opaque-profile"});
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_session_id = Some(old_thread.clone());
        tab.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: old_thread.clone(),
        };
        tab.agent_runtime_config = Some(config.clone());
    }
    crate::service::tab_chat::chatbackend_history::ChatBackendHistory::runtime_shared(
        PROVIDER,
        &old_thread,
    )
    .lock()
    .snapshot(saved.to_vec())
    .unwrap();
    (fixture, old_thread, config)
}

#[test]
fn lost_session_without_agent_work_starts_fresh_once_and_keeps_the_saved_transcript() {
    let unanswered = ChatMsg {
        id: "saved-prompt".into(),
        kind: "user".into(),
        content: "opaque unanswered prompt".into(),
        ..Default::default()
    };
    let notice = ChatMsg {
        id: "saved-notice".into(),
        kind: "system".into(),
        content: "opaque host notice".into(),
        seq: 1,
        ..Default::default()
    };
    let (fixture, old_thread, config) = refused_load_fixture(&[unanswered, notice]);
    assert!(
        agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config).is_err()
    );
    assert!(agent_runtime::lost_session::join_for_test(&fixture.tab));
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    let fresh_thread = tab.agent_session_id.clone().unwrap();
    assert_ne!(fresh_thread, old_thread);
    assert!(tab
        .context_epoch_archives
        .iter()
        .any(|archive| archive.session_id == old_thread));
    let opens = fixture
        .requests()
        .into_iter()
        .filter(|request| request["method"] == "openThread")
        .count();
    assert_eq!(opens, 1);
    assert!(!fixture
        .requests()
        .iter()
        .any(|request| request["method"] == "submitTurn"));
    let (rows, _) =
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &fixture.tab, 0).unwrap();
    assert_eq!(rows[0].content, "opaque unanswered prompt");
}

#[test]
fn lost_session_after_agent_work_keeps_the_owner_choice() {
    for kind in ["assistant", "tool_call"] {
        let saved = [
            ChatMsg {
                id: "saved-prompt".into(),
                kind: "user".into(),
                content: "opaque prompt".into(),
                ..Default::default()
            },
            ChatMsg {
                id: "saved-reply".into(),
                kind: kind.into(),
                content: "opaque reply".into(),
                seq: 1,
                ..Default::default()
            },
        ];
        let (fixture, old_thread, config) = refused_load_fixture(&saved);
        assert!(
            agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config)
                .is_err()
        );
        assert!(agent_runtime::lost_session::join_for_test(&fixture.tab));
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        assert!(agent_runtime::engine::session_load_refused(
            &fixture.service,
            &session
        ));
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&fixture.tab)
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some(old_thread.as_str())
        );
        assert!(!fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "openThread"));
    }
}

#[test]
fn focus_wake_of_a_lost_answered_session_shows_the_degraded_state_and_why() {
    let saved = [
        ChatMsg {
            id: "saved-prompt".into(),
            kind: "user".into(),
            content: "opaque prompt".into(),
            ..Default::default()
        },
        ChatMsg {
            id: "saved-reply".into(),
            kind: "assistant".into(),
            content: "opaque reply".into(),
            seq: 1,
            ..Default::default()
        },
    ];
    let (fixture, old_thread, config) = refused_load_fixture(&saved);
    assert!(
        agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config).is_err()
    );
    assert!(agent_runtime::lost_session::join_for_test(&fixture.tab));
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.restored_from_snapshot = true;
        tab.frozen = true;
        tab.agent_runtime_state = None;
    }

    let reason = fixture
        .service
        .wake_tab_on_request(&fixture.tab)
        .unwrap_err();

    assert!(reason.starts_with("restored_session_unavailable: session_missing:"));
    assert!(reason.contains(&old_thread));
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert_eq!(tab.agent_session_id.as_deref(), Some(old_thread.as_str()));
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed)
    );
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason.as_str()));
    let state = tab.agent_runtime_state.expect("published runtime state");
    assert_eq!(state["state"], "degraded");
    assert_eq!(state["failureCode"], "session_load_refused");
    let (rows, _) =
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &fixture.tab, 0).unwrap();
    assert!(rows.iter().any(|row| row.content == "opaque reply"));
    assert!(!fixture
        .requests()
        .iter()
        .any(|request| request["method"] == "openThread"));
}

#[test]
fn fresh_after_load_refusal_preserves_profile_settings_and_archived_transcript() {
    let saved = vec![
        ChatMsg {
            id: "saved-turn".into(),
            kind: "user".into(),
            content: "opaque earlier turn".into(),
            ..Default::default()
        },
        ChatMsg {
            id: "saved-thinking".into(),
            kind: "thinking".into(),
            content: "opaque interrupted thought".into(),
            seq: 1,
            ..Default::default()
        },
    ];
    let (fixture, old_thread, config) = refused_load_fixture(&saved);
    assert!(
        agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config).is_err()
    );
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(agent_runtime::engine::session_load_refused(
        &fixture.service,
        &session
    ));
    let process = session.process.lock().clone().unwrap();
    assert!(process.ended.borrow().is_none());
    assert!(process.client.ended().borrow().is_none());
    let requests_before = fixture.requests();
    let (attached, rows) =
        agent_runtime::transport::snapshot(&fixture.service, &fixture.tab).unwrap();
    assert!(Arc::ptr_eq(&session, &attached));
    // Delivery adds envelope segments to user rows; the restored identity and content are the oracle.
    let identity = |rows: serde_json::Value| -> Vec<serde_json::Value> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| {
                serde_json::json!({"id":row["id"],"type":row["type"],"content":row["content"],"seq":row["seq"]})
            })
            .collect()
    };
    assert_eq!(
        identity(serde_json::to_value(rows).unwrap()),
        identity(serde_json::to_value(saved).unwrap())
    );
    assert_eq!(fixture.requests(), requests_before);
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .agent_runtime_config
        .clone();
    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(outcome.agent_ready);
    assert!(!outcome.force);
    assert_ne!(outcome.session_id.as_deref(), Some(old_thread.as_str()));
    let requests = fixture.requests();
    let opens = requests
        .iter()
        .filter(|request| request["method"] == "openThread")
        .collect::<Vec<_>>();
    assert_eq!(opens.len(), 1);
    assert_eq!(opens[0]["params"]["settings"]["model"], "supplied-model");
    assert_eq!(opens[0]["params"]["settings"]["effort"], "opaque-effort");
    let mut expected_config = before.unwrap();
    expected_config["unusedThreadId"] = json!(outcome.session_id);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_config,
        Some(expected_config)
    );
    let (rows, _) =
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &fixture.tab, 0).unwrap();
    assert_eq!(rows[0].content, "opaque earlier turn");
    assert!(rows[0]
        .id
        .starts_with(&format!("provider-session:{old_thread}:")));
    assert_eq!(
        rows[2].system_subtype.as_deref(),
        Some("context_epoch_boundary")
    );
    let archive = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .context_epoch_archives[0]
        .clone();
    assert_eq!(
        archive.transport,
        crate::multiplexer::types::ContextEpochTransport::ChatBackend
    );
}

#[test]
fn start_new_session_after_a_failed_relaunch_opens_a_thread_and_archives_the_dead_one() {
    let saved = [
        ChatMsg {
            id: "saved-reply".into(),
            kind: "assistant".into(),
            content: "opaque earlier reply".into(),
            ..Default::default()
        },
        ChatMsg {
            id: "saved-tool".into(),
            kind: "tool_call".into(),
            content: "opaque cut-off tool".into(),
            tool_status: Some("running".into()),
            seq: 1,
            ..Default::default()
        },
    ];
    let (fixture, old_thread, config) = refused_load_fixture(&saved);
    assert!(
        agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config).is_err()
    );
    assert!(agent_runtime::lost_session::join_for_test(&fixture.tab));
    let count = |method: &str| {
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == method)
            .count()
    };
    // Automatic recovery relaunches the lost session once and fails, as a killed process does.
    assert!(fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .is_err());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed)
    );
    let loads = count("resumeThread");

    // Opening the tab retries nothing, so the explicit action below is not blocked by a relaunch.
    fixture.service.wake_tab_on_request(&fixture.tab).unwrap();
    assert_eq!(count("resumeThread"), loads);
    assert_eq!(count("openThread"), 0);

    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                retry_refused_start: true,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(outcome.agent_ready);
    assert_eq!(outcome.session_action(), "fresh_launch");
    assert_ne!(outcome.session_id.as_deref(), Some(old_thread.as_str()));
    assert_eq!(count("resumeThread"), loads);
    assert_eq!(count("openThread"), 1);
    let (rows, _) =
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &fixture.tab, 0).unwrap();
    assert_eq!(rows[0].content, "opaque earlier reply");
    assert_eq!(rows[1].content, "opaque cut-off tool");
    assert_eq!(rows[1].tool_status.as_deref(), Some("interrupted"));
    assert_eq!(
        rows[2].system_subtype.as_deref(),
        Some("context_epoch_boundary")
    );
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert!(tab
        .agent_runtime_state
        .as_ref()
        .is_some_and(|state| state["active_turn_started_at_ms"].is_null()));
    assert_eq!(
        tab.agent_state.map(|live| live.state),
        Some(codeterm_types::pty::TimelineState::Idle)
    );
}

#[test]
fn fresh_runtime_restart_still_refuses_a_loaded_live_session() {
    let fixture = RuntimeFixture::new();
    for code in [
        "session_load_refused",
        "runtime_start_unbound",
        "provider_refused",
    ] {
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&fixture.tab)
            .unwrap()
            .agent_runtime_state = Some(json!({"state":"degraded","failureCode":code}));
        let before = fixture.requests();
        assert!(fixture
            .service
            .agent_restart_observed_with_options(
                &fixture.tab,
                RestartOptions {
                    fresh: true,
                    ..Default::default()
                }
            )
            .is_err());
        assert_eq!(fixture.requests(), before);
        assert!(agent_runtime::engine::has_live_bound_session(
            fixture.service.mux.read().get_tab(&fixture.tab).unwrap()
        ));
    }
}
