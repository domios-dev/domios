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
    named_login_fixture(missing_secret, false)
}

fn named_login_fixture(
    missing_secret: bool,
    retained_thread: bool,
) -> (RuntimeFixture, AccountRestartTarget) {
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
        tab.reported_session_binding_nonce = None;
        if !retained_thread {
            tab.agent_session_id = None;
            if let TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
                session_id.clear();
            }
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
    let expected = if retained_thread {
        AccountRestartBinding::capture_refresh(&fixture.service, &fixture.tab)
    } else {
        AccountRestartBinding::capture_login(
            fixture.service.mux.read().get_tab(&fixture.tab).unwrap(),
            session.state.lock().epoch,
        )
    }
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

#[test]
fn expired_named_login_refresh_resumes_only_its_retained_thread() {
    for before_spawn in [false, true] {
        let (fixture, target) = named_login_fixture(before_spawn, true);
        assert!(target.expected.refresh_login && !target.expected.unbound_login);
        assert!(AccountRestartBinding::capture(
            fixture.service.mux.read().get_tab(&fixture.tab).unwrap(),
            target.expected.runtime_epoch,
        )
        .is_err());
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let old_process = session.process.lock().clone();
        if before_spawn {
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
        }
        let count = fixture.requests().len();
        let outcome = restart(&fixture, target).unwrap();
        assert!(outcome.agent_ready);
        assert_eq!(outcome.session_id.as_deref(), Some("own-thread"));
        if let Some(old) = old_process {
            assert!(old.ended.borrow().is_some());
        }
        let requests = fixture.requests();
        assert_eq!(
            requests[count..]
                .iter()
                .map(|request| request["method"].as_str().unwrap())
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
    }
}

#[test]
fn expired_named_login_refresh_refuses_stale_thread_epoch_and_cross_account() {
    for mutation in 0..4 {
        let (fixture, mut target) = named_login_fixture(false, true);
        match mutation {
            0 => target.expected.session_id = "unrelated-thread".into(),
            1 => {
                target.expected.runtime_epoch = target.expected.runtime_epoch.map(|epoch| epoch + 1)
            }
            2 => {
                target.account_id = Some("account-c".into());
                target.invocation = Some(
                    InvocationContext::new(
                        PROVIDER.into(),
                        "account-c".into(),
                        Default::default(),
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                );
            }
            _ => target.account_namespace = target.expected.account_namespace.clone(),
        }
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let process = session.process.lock().clone().unwrap();
        let count = fixture.requests().len();
        assert!(restart(&fixture, target).is_err());
        assert_eq!(fixture.requests().len(), count);
        assert!(process.ended.borrow().is_none());
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("own-nonce"));
        assert_eq!(
            tab.restorable_launch.account_namespace.as_deref(),
            Some("old-namespace")
        );
    }
}

#[test]
fn expired_named_login_refresh_requires_retained_host_restart_authority() {
    let (fixture, _) = named_login_fixture(false, true);
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    session.context.lock().binding_nonce = "unrecorded-nonce".into();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .session_binding_nonce = Some("unrecorded-nonce".into());
    assert!(agent_runtime::engine::validate_binding(&fixture.service, &session).is_ok());
    assert!(AccountRestartBinding::capture_refresh(&fixture.service, &fixture.tab).is_err());
}

fn authenticated_refusal_fixture() -> RuntimeFixture {
    let source = SOURCE.replace("    let result = {kind:'done'};", r#"
    if (op.kind === 'resumeThread' && reply.value.authKey === 'credential-b')
      return {kind:'refused',beforeAcceptance:true,reason:{code:'session_load_refused',message:'opaque authenticated target refusal'}};
    let result = {kind:'done'};
"#).replace("return {kind:'complete',result};", "return {kind:'complete',result,events:op.kind === 'initialize' ? [{kind:'auth',state:'signedIn'}] : []};");
    let fixture = RuntimeFixture::prepared(&source, true);
    crate::service::provider_accounts::test_support::configure(
        &fixture.service,
        fixture.temp.path().join("rollback-accounts"),
        fixture.temp.path().join("rollback-config.toml"),
    )
    .unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.restorable_launch.account_id = Some("account-a".into());
        tab.restorable_launch.account_namespace = Some("namespace-a".into());
    }
    fixture.service.ensure_agent_pad(&fixture.tab).unwrap();
    agent_runtime::authority::record_spawn_for_test(
        &fixture.service,
        &fixture.tab,
        PROVIDER,
        "own-nonce",
    );
    let invocation = InvocationContext::new(
        PROVIDER.into(),
        "account-a".into(),
        [("OPAQUE_AUTH_KEY".into(), Some("credential-a".into()))].into(),
        vec![],
        vec![],
    )
    .unwrap();
    crate::async_runtime::block_on(agent_runtime::engine::ensure_with_invocation(
        &fixture.service,
        &fixture.tab,
        Some(invocation),
    ))
    .unwrap();
    fixture
}

#[test]
fn authenticated_target_refusal_restores_exact_source_unless_loaded_or_input_arrived() {
    for mutation in 0..3 {
        let fixture = authenticated_refusal_fixture();
        let target = target(&fixture);
        let original = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let old_process = original.process.lock().clone().unwrap();
        let failed_process = Arc::new(parking_lot::Mutex::new(None));
        let captured_process = failed_process.clone();
        let new_input = Arc::new(parking_lot::Mutex::new(None));
        let captured_input = new_input.clone();
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let once = observed.clone();
        let owner = fixture.service.clone();
        let tab_id = fixture.tab.clone();
        let listener = fixture.ui.listen("agent-runtime-state", move |event| {
            if event["tab_id"] != tab_id
                || event["state"]["failureCode"] != "session_load_refused"
                || once.swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                return;
            }
            let failed = agent_runtime::engine::for_tab(&tab_id).unwrap();
            *captured_process.lock() = failed.process.lock().clone();
            if mutation == 1 {
                failed
                    .thread_loaded
                    .store(true, std::sync::atomic::Ordering::Release);
            } else if mutation == 2 {
                *captured_input.lock() = Some(
                    owner
                        .chat_send_ledger
                        .try_start(&tab_id, "opaque newer input", None)
                        .unwrap()
                        .send_id,
                );
            }
        });
        let count = fixture.requests().len();
        let error = restart(&fixture, target).unwrap_err();
        fixture.ui.unlisten(listener);
        assert!(observed.load(std::sync::atomic::Ordering::Acquire));
        let RestartAttemptError::Failed {
            outcome, timed_out, ..
        } = error
        else {
            panic!("target refusal was not a failed restart")
        };
        assert!(outcome.account_target_refused);
        assert!(outcome
            .account_refusal_nonce
            .as_deref()
            .is_some_and(|nonce| nonce != "own-nonce"));
        assert!(!timed_out);
        assert_eq!(outcome.source_account_restored, mutation == 0);
        let requests = fixture.requests();
        let methods = requests[count..]
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>();
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
        assert!(old_process.ended.borrow().is_some());
        if mutation == 0 {
            assert!(outcome.agent_ready && outcome.binding_restored);
            assert_ne!(outcome.account_refusal_nonce, tab.session_binding_nonce);
            assert_eq!(
                tab.restorable_launch.account_id.as_deref(),
                Some("account-a")
            );
            assert_eq!(
                tab.restorable_launch.account_namespace.as_deref(),
                Some("namespace-a")
            );
            assert_eq!(
                methods,
                vec![
                    "initialize",
                    "resumeThread",
                    "initialize",
                    "resumeThread",
                    "readHistory"
                ]
            );
            assert_eq!(requests[count + 1]["params"]["threadId"], "own-thread");
            assert_eq!(requests[count + 3]["params"]["threadId"], "own-thread");
            assert!(failed_process
                .lock()
                .as_ref()
                .unwrap()
                .ended
                .borrow()
                .is_some());
            assert_eq!(
                agent_runtime::process::test_ports::spawned_env(&fixture.tab)
                    .get("OPAQUE_AUTH_KEY")
                    .map(String::as_str),
                Some("credential-a")
            );
        } else {
            assert_eq!(
                tab.restorable_launch.account_id.as_deref(),
                Some("account-b")
            );
            assert_eq!(outcome.account_refusal_nonce, tab.session_binding_nonce);
            assert_eq!(methods, vec!["initialize", "resumeThread"]);
            assert!(failed_process
                .lock()
                .as_ref()
                .unwrap()
                .ended
                .borrow()
                .is_none());
            if let Some(id) = new_input.lock().as_ref() {
                assert!(fixture.service.chat_send_ledger.status(id).is_some());
            }
        }
    }
}
