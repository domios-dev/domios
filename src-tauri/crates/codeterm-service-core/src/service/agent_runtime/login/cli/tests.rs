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
        provider: "fixture".into(), account: "default".into(), nonce: Some("own-nonce".into()),
        state: json!({"state":"loginPending","auth":"signedOut","login":{"status":"pending","loginId":"own-login","url":"https://example.test/start"}}),
        stop: Some(stop), notification: None, input: None,
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
