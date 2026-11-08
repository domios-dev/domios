use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::hook_session_binding::{bind_reported_session, finish_session_binding};
use crate::service::test_fixture::ServiceFixture;
use crate::session::lifecycle::BindingSource;

const PROVIDER: &str = "resume-fixture";
const SESSION: &str = "own-session";
const NONCE: &str = "resumed-launch";
const MODEL: &str = "fixture-model";

/// The state `persist_restored_launch_identity` leaves: same session, new launch, no evidence.
fn resumed_launch(session_model: &str) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some("/fixture/project".into());
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some(SESSION.into());
    tab.session_binding_nonce = Some(NONCE.into());
    tab.agent_requested_model = Some(MODEL.into());
    tab.agent_provider_resolved_model = Some(MODEL.into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new(PROVIDER).with_session_model(SESSION, session_model),
    ));
    (fixture, tab_id)
}

async fn report_binding(fixture: &ServiceFixture, tab_id: &str, body: serde_json::Value) {
    let claim = bind_reported_session(
        &fixture.service.mux,
        &fixture.service.agent_registry,
        tab_id,
        PROVIDER,
        SESSION,
        Some(NONCE),
    )
    .unwrap();
    assert!(!claim.changed);
    assert!(claim.nonce_verified);
    finish_session_binding(
        &fixture.service,
        tab_id,
        PROVIDER,
        SESSION,
        Some(NONCE),
        None,
        "session.get",
        &body,
        claim,
        BindingSource::Hook,
        false,
    )
    .await
    .unwrap();
}

fn session_model(fixture: &ServiceFixture, tab_id: &str) -> Option<String> {
    fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .agent_session_model
        .clone()
}

#[tokio::test]
async fn resumed_launch_publishes_its_model_on_its_first_verified_report() {
    let (fixture, tab_id) = resumed_launch(MODEL);

    report_binding(&fixture, &tab_id, serde_json::Value::Null).await;

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_model.as_deref(), Some(MODEL));
    assert!(tab.model_identity_mismatch.is_none());
    let identity = tab.agent_session_model_identity.as_ref().unwrap();
    assert_eq!(identity.session_id, SESSION);
    assert_eq!(identity.session_binding_nonce, NONCE);
}

#[tokio::test]
async fn resumed_launch_reports_a_session_running_another_model_as_a_mismatch() {
    let (fixture, tab_id) = resumed_launch("other-model");

    report_binding(&fixture, &tab_id, serde_json::Value::Null).await;

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_model.as_deref(), Some("other-model"));
    assert_eq!(
        tab.model_identity_mismatch
            .as_ref()
            .map(|mismatch| mismatch.requested_model.as_str()),
        Some(MODEL)
    );
}

#[tokio::test]
async fn acknowledged_reports_of_the_same_launch_do_not_read_the_session_again() {
    let (fixture, tab_id) = resumed_launch(MODEL);
    report_binding(&fixture, &tab_id, serde_json::Value::Null).await;
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_model = None;
        tab.agent_session_model_identity = None;
    }

    report_binding(&fixture, &tab_id, serde_json::Value::Null).await;

    assert_eq!(session_model(&fixture, &tab_id), None);
}

#[tokio::test]
async fn a_model_the_launch_reported_outranks_the_session_record() {
    let (fixture, tab_id) = resumed_launch("record-before-launch");

    report_binding(&fixture, &tab_id, serde_json::json!({ "model": MODEL })).await;

    assert_eq!(session_model(&fixture, &tab_id).as_deref(), Some(MODEL));
}
