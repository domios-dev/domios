use super::*;

fn challenge() -> (Claim, RuntimeContext, RuntimeLoginState) {
    let claim = Claim {
        method: "fixture:oauth:2".into(),
        id: "own-login".into(),
        nonce: "own-nonce".into(),
        epoch: 3,
        thread_id: None,
        callback_started: false,
    };
    let context = RuntimeContext {
        binding_nonce: claim.nonce.clone(),
        epoch: claim.epoch,
        ..Default::default()
    };
    let login = serde_json::from_value(json!({"status":"pending","loginId":"own-login","url":"https://example.test/login","codeInput":false,"expiresAtMs":5000})).unwrap();
    (claim, context, login)
}

#[test]
fn challenge_requires_own_login_binding_epoch_and_unexpired_pending_state() {
    let (claim, context, login) = challenge();
    assert!(current_at(&claim, Some(&login), &context, 4999));
    assert!(!current_at(&claim, Some(&login), &context, 5000));
    for context in [
        RuntimeContext {
            epoch: 4,
            ..context.clone()
        },
        RuntimeContext {
            binding_nonce: "foreign".into(),
            ..context.clone()
        },
        RuntimeContext {
            thread_id: Some("foreign-thread".into()),
            ..context.clone()
        },
    ] {
        assert!(!current_at(&claim, Some(&login), &context, 0));
    }
    for login in [
        RuntimeLoginState {
            login_id: Some("foreign".into()),
            ..login.clone()
        },
        RuntimeLoginState {
            status: RuntimeLoginStatus::Canceled,
            ..login.clone()
        },
        RuntimeLoginState {
            status: RuntimeLoginStatus::Completed,
            ..login.clone()
        },
    ] {
        assert!(!current_at(&claim, Some(&login), &context, 0));
    }
    assert!(!current_at(&claim, None, &context, 0));
}

#[test]
fn callback_is_claimed_once_and_only_for_the_explicit_challenge_input() {
    let (mut claim, context, login) = challenge();
    assert!(!claim.begin_callback(Some(&login), &context, 0, true));
    assert!(!claim.callback_started);
    assert!(claim.begin_callback(Some(&login), &context, 0, false));
    assert!(!claim.begin_callback(Some(&login), &context, 1, false));
    let (mut claim, context, mut login) = challenge();
    login.code_input = None;
    assert!(!claim.begin_callback(Some(&login), &context, 0, false));
    login.code_input = Some(true);
    assert!(!claim.begin_callback(Some(&login), &context, 5000, true));
    assert!(claim.begin_callback(Some(&login), &context, 4999, true));
    assert!(!claim.begin_callback(Some(&login), &context, 4999, true));
}

#[test]
fn cancellation_bypasses_busy_gate_and_fences_late_completion() {
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .unwrap()
        .agent_runtime()
        .unwrap();
    let binary = capability.command.binary.clone();
    let mut manifest = json!({"id":session.provider,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":capability},"permissions":{"subprocess":{"allow":[binary]}}});
    manifest["capabilities"]["agentRuntime"]
        .as_object_mut()
        .unwrap()
        .remove("login");
    let provider = codeterm_plugin::js_runtime::JsProvider::new(
        codeterm_plugin::js_runtime::PluginManifest::from_json_str(&manifest.to_string()).unwrap(),
        Arc::new(
            codeterm_plugin::js_runtime::PluginVm::from_source(
                &session.provider,
                "module.exports={};".into(),
            )
            .unwrap(),
        ),
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    *session.adapter.lock() = fixture
        .service
        .agent_registry
        .provider_arc(&session.provider);
    assert!(session
        .adapter
        .lock()
        .as_ref()
        .unwrap()
        .agent_runtime()
        .unwrap()
        .login
        .is_none());
    let context = session.context.lock().clone();
    let (mut claim, _, mut login) = challenge();
    claim.nonce = context.binding_nonce.clone();
    claim.epoch = context.epoch;
    claim.thread_id = context.thread_id.clone();
    login.expires_at_ms = None;
    session.login.lock().declared = Some(claim.clone());
    session.login.lock().login = Some(login.clone());
    let permit = session.gate.try_acquire().unwrap();
    let request = serde_json::from_value(json!({"action":"cancel","loginId":claim.id,
        "binding":{"nonce":context.binding_nonce,"epoch":context.epoch}}))
    .unwrap();
    crate::async_runtime::block_on(super::super::operate(&fixture.service, &tab, request)).unwrap();
    assert_eq!(
        session.login.lock().login.as_ref().unwrap().status,
        RuntimeLoginStatus::Canceled
    );
    assert!(validate_witness(&session, &(claim.method, claim.id)).is_err());
    login.status = RuntimeLoginStatus::Completed;
    super::super::apply(&fixture.service, &session, context.epoch, login);
    assert_eq!(
        session.login.lock().login.as_ref().unwrap().status,
        RuntimeLoginStatus::Canceled
    );
    assert_eq!(session.context.lock().thread_id, context.thread_id);
    drop(permit);
}
