use super::*;
use crate::service::agent_lifecycle::{AccountRestartBinding, AccountRestartTarget};
use codeterm_plugin::provider_account::InvocationContext;

fn target(fixture: &RuntimeFixture) -> AccountRestartTarget {
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let epoch = session.state.lock().epoch;
    let expected = AccountRestartBinding::capture(
        fixture.service.mux.read().get_tab(&fixture.tab).unwrap(),
        Some(epoch),
    )
    .unwrap();
    AccountRestartTarget {
        terminal_turn: None,
        expected,
        account_id: Some("account-b".into()),
        account_namespace: Some("namespace-b".into()),
        invocation: Some(
            InvocationContext::new(
                PROVIDER.into(),
                "account-b".into(),
                [
                    ("OPAQUE_AUTH_KEY".into(), Some("credential-b".into())),
                    ("OPAQUE_OLD_KEY".into(), None),
                ]
                .into(),
                vec![],
                vec![],
            )
            .unwrap(),
        ),
    }
}

fn restart(
    fixture: &RuntimeFixture,
    target: AccountRestartTarget,
) -> Result<RestartOutcome, RestartAttemptError> {
    fixture.service.agent_restart_observed_with_options(
        &fixture.tab,
        RestartOptions {
            account_restart: Some(target),
            ..Default::default()
        },
    )
}

fn unbound_login() -> (RuntimeFixture, AccountRestartTarget) {
    unbound_login_fixture(false)
}

fn unbound_login_fixture(missing_secret: bool) -> (RuntimeFixture, AccountRestartTarget) {
    let source = SOURCE.replace("    let result = {kind:'done'};", r#"
    if ((op.kind === 'initialize' || (op.kind === 'control' && op.action === 'auth')) && reply.value.authKey !== 'credential-b')
      return {kind:'complete',result:{kind:op.kind === 'initialize' ? 'initialized' : 'done'},events:[{kind:'auth',state:'signedOut'}]};
    let result = {kind:'done'};
"#);
    let fixture = RuntimeFixture::prepared_with_profile(&source, true, missing_secret);
    if missing_secret {
        crate::async_runtime::block_on(async {
            *fixture.service.secrets.write().await =
                Arc::new(crate::secrets::mock::MockStore::new());
        });
    }
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        if missing_secret {
            tab.agent_runtime_config.as_mut().unwrap()["launchProfile"] = json!("credential");
        }
        tab.restorable_launch.account_id = Some("account-b".into());
        tab.restorable_launch.account_namespace = Some("old-namespace".into());
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        if let TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
            session_id.clear();
        }
    }
    fixture.service.ensure_agent_pad(&fixture.tab).unwrap();
    agent_runtime::authority::record_spawn_for_test(
        &fixture.service,
        &fixture.tab,
        PROVIDER,
        "own-nonce",
    );
    let old = InvocationContext::new(
        PROVIDER.into(),
        "account-b".into(),
        Default::default(),
        vec![],
        vec![],
    )
    .unwrap();
    let result = crate::async_runtime::block_on(agent_runtime::engine::ensure_with_invocation(
        &fixture.service,
        &fixture.tab,
        Some(old),
    ));
    assert!(matches!(
        result,
        Err(agent_runtime::model::RuntimeFailure::SignInRequired)
    ));
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(matches!(
        session.state.lock().phase,
        agent_runtime::model::RuntimePhase::LoginPending
    ));
    let expected = AccountRestartBinding::capture_login(
        fixture.service.mux.read().get_tab(&fixture.tab).unwrap(),
        session.state.lock().epoch,
    )
    .unwrap();
    let target = AccountRestartTarget {
        terminal_turn: None,
        expected,
        account_id: Some("account-b".into()),
        account_namespace: Some("new-namespace".into()),
        invocation: Some(
            InvocationContext::new(
                PROVIDER.into(),
                "account-b".into(),
                [("OPAQUE_AUTH_KEY".into(), Some("credential-b".into()))].into(),
                vec![],
                vec![],
            )
            .unwrap(),
        ),
    };
    (fixture, target)
}

#[test]
fn initial_named_login_replaces_credentials_before_opening_its_only_thread() {
    let (fixture, target) = unbound_login();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let old = session.process.lock().clone().unwrap();
    let count = fixture.requests().len();
    let outcome = restart(&fixture, target).unwrap();
    assert!(outcome.agent_ready && outcome.session_id.is_some());
    assert!(old.ended.borrow().is_some());
    assert_eq!(
        fixture.requests()[count..]
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["initialize", "openThread", "readHistory"]
    );
    assert_eq!(
        agent_runtime::process::test_ports::spawned_env(&fixture.tab)
            .get("OPAQUE_AUTH_KEY")
            .map(String::as_str),
        Some("credential-b")
    );
}

#[test]
fn initial_named_login_refuses_any_delivery_record_before_mutation() {
    for bootstrap in [false, true] {
        let (fixture, target) = unbound_login();
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let old = session.process.lock().clone().unwrap();
        let pending = if bootstrap {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&fixture.tab)
                .unwrap()
                .pending_delivery = Some(crate::multiplexer::types::PendingDeliveryRecord::new(
                "opaque payload".into(),
                crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
            ));
            None
        } else {
            Some(
                fixture
                    .service
                    .chat_send_ledger
                    .try_start(&fixture.tab, "opaque payload", None)
                    .unwrap(),
            )
        };
        let count = fixture.requests().len();
        assert!(restart(&fixture, target).is_err());
        assert_eq!(fixture.requests().len(), count);
        assert!(old.ended.borrow().is_none());
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("own-nonce"));
        assert_eq!(
            tab.restorable_launch.account_namespace.as_deref(),
            Some("old-namespace")
        );
        if let Some(pending) = pending {
            assert!(fixture
                .service
                .chat_send_ledger
                .status(&pending.send_id)
                .is_some());
        } else {
            assert!(tab.pending_delivery.is_some());
        }
    }
}

#[test]
fn account_restart_resumes_exact_thread_with_new_environment_once_without_input_replay() {
    let fixture = RuntimeFixture::new();
    let target = target(&fixture);
    let mut original = crate::multiplexer::types::PendingDeliveryRecord::new(
        "opaque already-written input".into(),
        crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
    )
    .with_send_id("opaque-input-send".into());
    original
        .begin_attempt(crate::service::launch_delivery::next_attempt())
        .unwrap();
    original.mark_unconfirmed();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .pending_delivery = Some(original);
    let count = fixture.requests().len();
    let prior = agent_runtime::engine::for_tab(&fixture.tab)
        .unwrap()
        .process
        .lock()
        .clone()
        .unwrap();
    let outcome = restart(&fixture, target.clone()).unwrap();
    assert!(prior.ended.borrow().is_some());
    assert!(outcome.agent_ready);
    assert_eq!(outcome.session_id.as_deref(), Some("own-thread"));
    let requests = fixture.requests();
    assert_eq!(
        requests[count..]
            .iter()
            .map(|frame| frame["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["initialize", "resumeThread", "readHistory"]
    );
    assert_eq!(requests[count + 1]["params"]["threadId"], "own-thread");
    assert_eq!(
        agent_runtime::process::test_ports::spawned_env(&fixture.tab)
            .get("OPAQUE_AUTH_KEY")
            .map(String::as_str),
        Some("credential-b")
    );
    assert!(
        !agent_runtime::process::test_ports::spawned_env(&fixture.tab)
            .contains_key("OPAQUE_OLD_KEY")
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(
            tab.restorable_launch.account_id.as_deref(),
            Some("account-b")
        );
        assert_eq!(
            tab.restorable_launch.account_namespace.as_deref(),
            Some("namespace-b")
        );
    }
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
    assert!(restart(&fixture, target).is_err());
    assert_eq!(fixture.requests().len(), requests.len());
}

#[test]
fn account_restart_refuses_busy_epoch_before_mutating_or_launching() {
    let fixture = RuntimeFixture::new();
    let target = target(&fixture);
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    session.context.lock().active_turn_id = Some("active-turn".into());
    let count = fixture.requests().len();
    let nonce = target.expected.binding_nonce.clone();
    assert!(restart(&fixture, target).is_err());
    assert_eq!(fixture.requests().len(), count);
    assert_eq!(session.context.lock().binding_nonce, nonce);
    assert_eq!(
        session.context.lock().active_turn_id.as_deref(),
        Some("active-turn")
    );
    assert!(session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .restorable_launch
        .account_id
        .is_none());
}

#[test]
fn account_restart_requires_old_process_cleanup_authority() {
    let fixture = RuntimeFixture::new();
    let target = target(&fixture);
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let old = session.process.lock().take();
    let count = fixture.requests().len();
    assert!(restart(&fixture, target).is_err());
    assert_eq!(fixture.requests().len(), count);
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .restorable_launch
        .account_id
        .is_none());
    *session.process.lock() = old;
}

#[test]
fn account_restart_same_account_credential_replacement_and_default_return_share_resume_path() {
    let fixture = RuntimeFixture::new();
    restart(&fixture, target(&fixture)).unwrap();
    for replacement in [true, false] {
        fixture
            .service
            .ext::<crate::service::agent_restart::RestartTracker>()
            .lock()
            .last_restart
            .remove(&fixture.tab);
        let mut target = target(&fixture);
        if replacement {
            target.account_namespace = Some("replacement-namespace".into());
            target.invocation = Some(
                InvocationContext::new(
                    PROVIDER.into(),
                    "account-b".into(),
                    [("OPAQUE_AUTH_KEY".into(), Some("replacement-key".into()))].into(),
                    vec![],
                    vec![],
                )
                .unwrap(),
            );
        } else {
            target.account_id = None;
            target.account_namespace = None;
            target.invocation = None;
        }
        let count = fixture.requests().len();
        let outcome = restart(&fixture, target).unwrap();
        assert_eq!(outcome.session_id.as_deref(), Some("own-thread"));
        let requests = fixture.requests();
        assert_eq!(
            requests[count..]
                .iter()
                .map(|frame| frame["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["initialize", "resumeThread", "readHistory"]
        );
        let env = agent_runtime::process::test_ports::spawned_env(&fixture.tab);
        assert_eq!(
            env.get("OPAQUE_AUTH_KEY").map(String::as_str),
            replacement.then_some("replacement-key")
        );
    }
}

#[test]
fn initial_named_login_before_any_child_spawn_uses_the_same_restart_path() {
    let (fixture, target) = unbound_login_fixture(true);
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(session.process.lock().is_none());
    assert!(fixture.requests().is_empty());
    crate::async_runtime::block_on(async {
        fixture
            .service
            .secrets
            .read()
            .await
            .clone()
            .save(crate::secrets::SaveRequest {
                name: "domios:provider:restart-fixture:key".into(),
                value: crate::secrets::SecretString::new("opaque-test-key".into()),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .unwrap();
    });
    let outcome = restart(&fixture, target).unwrap();
    assert!(outcome.agent_ready && outcome.session_id.is_some());
    assert_eq!(
        fixture
            .requests()
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["initialize", "openThread", "readHistory"]
    );
}
