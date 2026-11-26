use super::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::TabType},
    service::test_fixture::ServiceFixture,
};

fn pending() -> (ServiceFixture, String, watch::Receiver<bool>) {
    let (mut mux, tab) = MultiplexerState::new();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.tab_type = TabType::Choice;
    target.session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let (stop, stopping) = watch::channel(false);
    JOBS.lock().insert(tab.clone(), Job {
        installation_id: None,
        provider: "fixture".into(),
        account: "default".into(),
        session_id: None,
        nonce: Some("own-nonce".into()),
        epoch: 1,
        expires_at_ms: Some(now_ms().saturating_add(60_000)),
        code_submission: None,
        state: json!({
            "state": "loginPending",
            "epoch": 1,
            "auth": "signedOut",
            "loginCodeInput": true,
            "loginBinding": {"tabId": tab, "providerId": "fixture", "nonce": "own-nonce", "epoch": 1},
            "login": {"status": "pending", "loginId": "own-login", "url": "https://example.test/start"}
        }),
        stop: Some(stop),
        notification: None,
        input: None,
    });
    (fixture, tab, stopping)
}

#[tokio::test]
async fn account_cleanup_waits_for_the_owned_cli_process_receipt() {
    let (_fixture, tab, mut stopping) = pending();
    let (completed, finished) = watch::channel(false);
    let stop = JOBS.lock().get(&tab).unwrap().stop.clone().unwrap();
    RUNS.lock().insert(
        tab.clone(),
        Arc::new(Run {
            id: "own-login".into(),
            provider: "fixture".into(),
            account: "default".into(),
            nonce: Some("own-nonce".into()),
            stop,
            finished,
        }),
    );
    let closing = close_and_wait(&tab, "fixture", "default", Some("own-nonce"));
    tokio::pin!(closing);
    tokio::select! {
        biased;
        result = &mut closing => panic!("cleanup finished without a process receipt: {result:?}"),
        _ = std::future::ready(()) => {}
    }
    stopping.changed().await.unwrap();
    assert!(*stopping.borrow());
    assert!(JOBS.lock().contains_key(&tab));
    completed.send_replace(true);
    closing.await.unwrap();
    assert!(!JOBS.lock().contains_key(&tab));
    assert!(!RUNS.lock().contains_key(&tab));
}

#[tokio::test]
async fn account_cleanup_does_not_treat_a_dropped_worker_as_completion() {
    let (_fixture, tab, _stopping) = pending();
    let (completed, finished) = watch::channel(false);
    let stop = JOBS.lock().get(&tab).unwrap().stop.clone().unwrap();
    RUNS.lock().insert(
        tab.clone(),
        Arc::new(Run {
            id: "own-login".into(),
            provider: "fixture".into(),
            account: "default".into(),
            nonce: Some("own-nonce".into()),
            stop,
            finished,
        }),
    );
    drop(completed);
    close(&tab);
    assert!(!JOBS.lock().contains_key(&tab));
    assert!(
        close_and_wait(&tab, "fixture", "default", Some("own-nonce"))
            .await
            .is_err()
    );
    assert!(RUNS.lock().contains_key(&tab));
    RUNS.lock().remove(&tab);
}

#[tokio::test]
async fn account_cleanup_retains_process_ownership_after_ui_job_removal() {
    let (_fixture, tab, _stopping) = pending();
    let (completed, finished) = watch::channel(false);
    let stop = JOBS.lock().get(&tab).unwrap().stop.clone().unwrap();
    RUNS.lock().insert(
        tab.clone(),
        Arc::new(Run {
            id: "own-login".into(),
            provider: "fixture".into(),
            account: "default".into(),
            nonce: Some("own-nonce".into()),
            stop,
            finished,
        }),
    );
    close(&tab);
    let closing = close_and_wait(&tab, "fixture", "default", Some("own-nonce"));
    tokio::pin!(closing);
    tokio::select! { biased;
        result=&mut closing=>panic!("UI removal was mistaken for process cleanup: {result:?}"),
        _=std::future::ready(())=>{}
    }
    completed.send_replace(true);
    closing.await.unwrap();
    assert!(!RUNS.lock().contains_key(&tab));
}

#[test]
fn cli_login_pending_publication_reaches_the_runtime_event_bus() {
    let (fixture, tab, _stopping) = pending();
    let mut events = fixture.service.events.subscribe();
    publish(&fixture.service, &tab);
    let data = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event.event {
            codeterm_events::SystemEvent::Generic { event_type, data }
                if event_type == "agent-runtime-state" && data["tab_id"] == tab =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("runtime state reaches the bus synchronously");
    assert_eq!(data["tab_id"], tab);
    assert_eq!(data["session_binding_nonce"], "own-nonce");
    assert_eq!(data["state"]["state"], "loginPending");
    assert_eq!(data["state"]["auth"], "signedOut");
    close(&tab);
}

#[test]
fn cli_login_requires_own_completion_and_verified_authentication() {
    let (fixture, tab, _stopping) = pending();
    finish(&fixture.service, &tab, "foreign-login", Ok(true));
    assert_eq!(
        show(&fixture.service, &tab).unwrap()["state"]["auth"],
        "signedOut"
    );
    finish(&fixture.service, &tab, "own-login", Ok(false));
    let failure = show(&fixture.service, &tab).unwrap();
    assert_eq!(failure["state"]["auth"], "signedOut");
    assert_eq!(failure["login"]["status"], "failed");
    assert!(failure["login"].get("url").is_none());
    close(&tab);
}

#[test]
fn cli_login_rejects_stale_tab_binding_and_close_cancels_process_owner() {
    let (fixture, tab, mut stopping) = pending();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("new-nonce".into());
    observe(
        &fixture.service,
        &tab,
        "own-login",
        "https://example.test/stale".into(),
    );
    assert!(show(&fixture.service, &tab).is_err());
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_state
        .is_none());
    close(&tab);
    crate::async_runtime::block_on(async {
        assert!(stopping.changed().await.is_err());
    });
}

#[test]
fn cli_login_canceled_attempt_cannot_become_signed_in() {
    let (fixture, tab, _stopping) = pending();
    JOBS.lock().get_mut(&tab).unwrap().state["login"] =
        json!({"status":"canceled","loginId":"own-login"});
    finish(&fixture.service, &tab, "own-login", Ok(true));
    assert_eq!(
        show(&fixture.service, &tab).unwrap()["state"]["auth"],
        "signedOut"
    );
    close(&tab);
}

#[test]
fn cli_login_verified_completion_clears_link_and_code() {
    let (fixture, tab, _stopping) = pending();
    finish(&fixture.service, &tab, "own-login", Ok(true));
    let ready = show(&fixture.service, &tab).unwrap();
    assert_eq!(ready["state"]["auth"], "signedIn");
    assert_eq!(ready["login"]["status"], "completed");
    assert!(ready["login"].get("url").is_none());
    close(&tab);
}

#[test]
fn cli_login_old_account_cannot_publish_or_complete_under_the_same_nonce() {
    let (fixture, tab, mut stopping) = pending();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .account_id = Some("account-b".into());
    finish(&fixture.service, &tab, "own-login", Ok(true));
    observe(
        &fixture.service,
        &tab,
        "own-login",
        "https://example.test/late".into(),
    );
    assert!(show(&fixture.service, &tab).is_err());
    assert_eq!(JOBS.lock()[&tab].state["auth"], "signedOut");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .agent_runtime_state
        .is_none());
    cleanup(&fixture.service);
    assert!(!has(&tab));
    crate::async_runtime::block_on(async {
        stopping.changed().await.unwrap();
        assert!(*stopping.borrow_and_update());
    });
}

#[test]
fn cli_follow_up_requires_the_current_challenge_id() {
    let (fixture, tab, _stopping) = pending();
    for login_id in [None, Some("foreign-login"), Some("own-login")] {
        let request: LoginRequest =
            serde_json::from_value(json!({"action":"cancel","loginId":login_id})).unwrap();
        let jobs = JOBS.lock();
        assert_eq!(
            require_challenge(&fixture.service, &tab, &jobs[&tab], &request).is_ok(),
            login_id == Some("own-login")
        );
    }
    close(&tab);
}

fn register_cli_login_provider(fixture: &ServiceFixture) {
    let manifest = json!({
        "id": "fixture",
        "version": "1.0.0",
        "displayName": "Fixture CLI",
        "cliLogin": {
            "methods": [
                {"id": "subscription", "label": "Subscription", "args": ["auth", "login"]}
            ],
            "urlPattern": "(https://example\\.test/[^\\s]+)\\s",
            "timeoutMs": 60000,
            "codeInput": true
        },
        "capabilities": {
            "agentProvider": true
        }
    });
    let provider = codeterm_plugin::js_runtime::JsProvider::new(
        codeterm_plugin::js_runtime::PluginManifest::from_json_str(&manifest.to_string()).unwrap(),
        Arc::new(
            codeterm_plugin::js_runtime::PluginVm::from_source(
                "fixture",
                "module.exports={};".into(),
            )
            .unwrap(),
        ),
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
}

fn pending_with_code_channel(
    epoch: u64,
    expires_at_ms: u64,
) -> (ServiceFixture, String, mpsc::Receiver<CodeSubmission>) {
    let (mut mux, tab) = MultiplexerState::new();
    {
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("fixture".into());
        target.agent_session_id = Some("own-session".into());
        target.session_binding_nonce = Some("own-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    register_cli_login_provider(&fixture);
    let (stop, _stopping) = watch::channel(false);
    let (input, codes) = mpsc::channel(1);
    JOBS.lock().insert(
        tab.clone(),
        Job {
            installation_id: Some("inst-target".into()),
            provider: "fixture".into(),
            account: "default".into(),
            session_id: Some("own-session".into()),
            nonce: Some("own-nonce".into()),
            epoch,
            expires_at_ms: Some(expires_at_ms),
            code_submission: None,
            state: json!({
                "state": "loginPending",
                "epoch": epoch,
                "auth": "signedOut",
                "loginCodeInput": true,
                "loginBinding": {
                    "installationId": "inst-target",
                    "tabId": tab,
                    "providerId": "fixture",
                    "sessionId": "own-session",
                    "nonce": "own-nonce",
                    "epoch": epoch,
                },
                "login": {
                    "status": "pending",
                    "loginId": "flow-current",
                    "method": "browser",
                    "url": "https://example.test/oauth/authorize?state=current",
                    "expiresAtMs": expires_at_ms,
                }
            }),
            stop: Some(stop),
            notification: None,
            input: Some(input),
        },
    );
    (fixture, tab, codes)
}

#[tokio::test]
async fn cli_code_submission_rejects_binding_mismatch_stale_epoch_expired_and_different_flow() {
    let (fixture, tab, mut codes) = pending_with_code_channel(7, now_ms().saturating_add(60_000));

    let mismatched_cases = [
        (
            "wrong installation",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-other", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "missing installation",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "wrong tab",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": "foreign-tab", "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "wrong provider",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "other-provider", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "wrong session",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "foreign-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "wrong nonce",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "foreign-nonce", "epoch": 7}
            }),
        ),
        (
            "stale epoch",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 6}
            }),
        ),
        (
            "different login flow id (URL correlation hazard)",
            json!({
                "action": "code", "code": "one-time-pkce-code", "loginId": "flow-other-pty-url", "providerId": "fixture",
                "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 7}
            }),
        ),
        (
            "missing binding",
            json!({"action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture"}),
        ),
    ];

    for (label, body) in mismatched_cases {
        let request: LoginRequest = serde_json::from_value(body).unwrap();
        let err = operate(&fixture.service, &tab, request)
            .await
            .expect_err(label);
        assert!(
            !err.contains("one-time-pkce-code"),
            "{label}: error must not echo code"
        );
        assert!(
            codes.try_recv().is_err(),
            "{label}: mismatched request must never reach provider stdin"
        );
    }
    close(&tab);

    // Expired flow must also be rejected before touching stdin.
    let (expired_fixture, expired_tab, mut expired_codes) =
        pending_with_code_channel(8, now_ms().saturating_sub(1));
    let request: LoginRequest = serde_json::from_value(json!({
        "action": "code", "code": "one-time-pkce-code", "loginId": "flow-current", "providerId": "fixture",
        "binding": {"installationId": "inst-target", "tabId": expired_tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 8}
    }))
    .unwrap();
    assert!(operate(&expired_fixture.service, &expired_tab, request)
        .await
        .is_err());
    assert!(expired_codes.try_recv().is_err());
    close(&expired_tab);
}

#[tokio::test]
async fn cli_code_submission_writes_once_and_refuses_replay_after_uncertainty() {
    // 1. Accepted submission writes once and blocks replay.
    let (fixture, tab, mut codes) = pending_with_code_channel(10, now_ms().saturating_add(60_000));
    let make_req = || -> LoginRequest {
        serde_json::from_value(json!({
            "action": "code", "code": "secret-accepted-code", "loginId": "flow-current", "providerId": "fixture",
            "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 10}
        }))
        .unwrap()
    };
    let receiver = tokio::spawn(async move {
        let (code, receipt) = codes.recv().await.expect("receives code once");
        assert_eq!(code.as_str(), "secret-accepted-code");
        receipt.send(process::CodeWriteOutcome::Written).unwrap();
        assert!(
            codes.recv().await.is_none(),
            "channel closes after one send"
        );
    });
    let accepted = operate(&fixture.service, &tab, make_req()).await.unwrap();
    assert_eq!(accepted["outcome"], "accepted");
    assert_eq!(accepted["codeSubmission"]["status"], "accepted");
    receiver.await.unwrap();
    let replay = operate(&fixture.service, &tab, make_req()).await;
    assert!(replay.is_err(), "replay after accepted must be refused");
    close(&tab);

    // 2. Uncertain delivery returns typed uncertain outcome and refuses replay.
    let (fixture, tab, mut codes) = pending_with_code_channel(11, now_ms().saturating_add(60_000));
    let make_req = || -> LoginRequest {
        serde_json::from_value(json!({
            "action": "code", "code": "secret-uncertain-code", "loginId": "flow-current", "providerId": "fixture",
            "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 11}
        }))
        .unwrap()
    };
    let receiver = tokio::spawn(async move {
        let (code, receipt) = codes.recv().await.expect("receives code once");
        assert_eq!(code.as_str(), "secret-uncertain-code");
        receipt
            .send(process::CodeWriteOutcome::Uncertain(
                "flush unconfirmed".into(),
            ))
            .unwrap();
        assert!(codes.recv().await.is_none(), "no replay reaches stdin");
    });
    let uncertain = operate(&fixture.service, &tab, make_req()).await.unwrap();
    assert_eq!(uncertain["outcome"], "uncertain");
    assert_eq!(uncertain["codeSubmission"]["status"], "uncertain");
    receiver.await.unwrap();
    let replay = operate(&fixture.service, &tab, make_req())
        .await
        .expect_err("replay after uncertainty must be refused");
    assert!(replay.contains("uncertain"), "{replay}");
    close(&tab);
}

#[tokio::test]
async fn cli_code_submission_never_leaks_secret_to_logs_events_or_state() {
    struct CaptureLogs(Mutex<Vec<String>>);
    impl log::Log for CaptureLogs {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().push(format!("{}", record.args()));
        }
        fn flush(&self) {}
    }
    static CAPTURE_LOGS: LazyLock<CaptureLogs> =
        LazyLock::new(|| CaptureLogs(Mutex::new(Vec::new())));

    let _ = log::set_logger(&*CAPTURE_LOGS);
    log::set_max_level(log::LevelFilter::Trace);
    CAPTURE_LOGS.0.lock().clear();

    let sentinel = "pkce-sentinel-secret-9f4e2a1b";
    let (fixture, tab, mut codes) = pending_with_code_channel(12, now_ms().saturating_add(60_000));
    let mut bus_events = fixture.service.events.subscribe();

    // 1. Trigger logged refusal through top-level operate() (wrong epoch -> log::warn!).
    let bad_req: LoginRequest = serde_json::from_value(json!({
        "action": "code", "code": sentinel, "loginId": "flow-current", "providerId": "fixture",
        "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 1}
    }))
    .unwrap();
    let refused = super::super::operate(&fixture.service, &tab, bad_req)
        .await
        .unwrap_err();
    assert!(!refused.contains(sentinel));

    // 2. Trigger accepted submission through top-level operate().
    let good_req: LoginRequest = serde_json::from_value(json!({
        "action": "code", "code": sentinel, "loginId": "flow-current", "providerId": "fixture",
        "binding": {"installationId": "inst-target", "tabId": tab, "providerId": "fixture", "sessionId": "own-session", "nonce": "own-nonce", "epoch": 12}
    }))
    .unwrap();
    let receiver = tokio::spawn(async move {
        let (code, receipt) = codes.recv().await.unwrap();
        assert_eq!(code.as_str(), sentinel);
        receipt.send(process::CodeWriteOutcome::Written).unwrap();
    });
    let response = super::super::operate(&fixture.service, &tab, good_req)
        .await
        .unwrap();
    receiver.await.unwrap();

    // Scan captured logs, HTTP response, show(), mux state, UI events, and event bus.
    let logs = CAPTURE_LOGS.0.lock().join("\n");
    assert!(!logs.contains(sentinel), "secret leaked into logs: {logs}");
    assert!(
        !response.to_string().contains(sentinel),
        "secret leaked into response"
    );
    let shown = show(&fixture.service, &tab).unwrap();
    assert!(
        !shown.to_string().contains(sentinel),
        "secret leaked into show()"
    );
    let mux_state = serde_json::to_string(
        &fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .agent_runtime_state,
    )
    .unwrap();
    assert!(
        !mux_state.contains(sentinel),
        "secret leaked into mux agent_runtime_state"
    );
    let ui_events = serde_json::to_string(&*fixture.ui.0.lock()).unwrap();
    assert!(
        !ui_events.contains(sentinel),
        "secret leaked into UI events"
    );
    while let Ok(event) = bus_events.try_recv() {
        let serialized = format!("{event:?}");
        assert!(
            !serialized.contains(sentinel),
            "secret leaked into system event bus"
        );
    }
    close(&tab);
}
