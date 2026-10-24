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
        provider: "fixture".into(), nonce: Some("own-nonce".into()),
        state: json!({"state":"loginPending","auth":"signedOut","login":{"status":"pending","loginId":"own-login","url":"https://example.test/start"}}),
        stop: Some(stop), notification: None, input: None,
    });
    (fixture, tab, stopping)
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
