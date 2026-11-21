use super::sync_send::{
    runtime_fixture_with_stored_history, runtime_fixture_without_remote_history,
    RUNTIME_FIXTURE_SOURCE,
};
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn observed_target(
    fixture: &crate::service::test_fixture::ServiceFixture,
    source_id: &str,
    restored: bool,
) -> crate::multiplexer::types::Tab {
    let expected = {
        let mut mux = fixture.service.mux.write();
        let source = mux.get_tab(source_id).unwrap().clone();
        let workspace = mux.workspaces[0].id.clone();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = source.agent_type.clone();
        tab.cwd = source.cwd;
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: source.agent_type.unwrap(),
            session_id: String::new(),
        };
        tab.restored_from_snapshot = restored;
        tab.clone()
    };
    let (nonce, committed) = fixture
        .service
        .reserve_agent_launch_binding_nonce_if_current(&expected, None, None)
        .unwrap();
    crate::service::agent_runtime::authority::record_spawn(
        &fixture.service,
        &committed.id,
        committed.agent_type.as_deref().unwrap(),
        nonce.as_deref().unwrap(),
    )
    .unwrap();
    committed
}

#[test]
fn completed_original_refusal_has_a_single_consuming_call_bound_observation() {
    for fork in [None, Some(("source-thread", "source-turn"))] {
        for comparison in [
            "original",
            "other-call",
            "replacement",
            "other-fork",
            "other-host",
        ] {
            let (fixture, source, _peer, _cleanup) =
                runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
            let original = observed_target(&fixture, &source, false);
            let expected = original.clone();
            crate::service::agent_runtime::authority::record_spawn(
                &fixture.service,
                &original.id,
                original.agent_type.as_deref().unwrap(),
                original.session_binding_nonce.as_deref().unwrap(),
            )
            .unwrap();
            let replacement = Arc::new(Mutex::new(None));
            let captured = replacement.clone();
            assert!(fixture
                .service
                .set_ext(ExpectedLaunchCommit {
                    stage: "account_provision",
                    callback: Mutex::new(Some(Box::new(move |svc| {
                        let (_, tab) = svc
                            .reserve_agent_launch_binding_nonce_if_current(&expected, None, None)
                            .unwrap();
                        crate::service::agent_runtime::authority::record_spawn(
                            svc,
                            &tab.id,
                            tab.agent_type.as_deref().unwrap(),
                            tab.session_binding_nonce.as_deref().unwrap(),
                        )
                        .unwrap();
                        *captured.lock() = Some(serde_json::to_value(tab).unwrap());
                    })))
                })
                .is_ok());
            let mut outcome = crate::async_runtime::block_on(ensure_expected_observed(
                &fixture.service,
                &original.id,
                original.session_binding_nonce.as_deref().unwrap(),
                fork,
            ));
            let witness = outcome
                .take_rejection()
                .expect("completed account refusal before original session acquisition");
            assert!(outcome.take_rejection().is_none());
            if comparison == "other-call" {
                let other = crate::async_runtime::block_on(ensure_expected_observed(
                    &fixture.service,
                    &original.id,
                    original.session_binding_nonce.as_deref().unwrap(),
                    fork,
                ));
                assert!(!witness.matches_original(&other, &fixture.service, &original, fork));
            } else if comparison == "replacement" {
                let replacement = fixture
                    .service
                    .mux
                    .read()
                    .get_tab(&original.id)
                    .unwrap()
                    .clone();
                assert!(!witness.matches_original(&outcome, &fixture.service, &replacement, fork));
            } else if comparison == "other-fork" {
                let other = if fork.is_some() {
                    None
                } else {
                    Some(("other-source", "other-turn"))
                };
                assert!(!witness.matches_original(&outcome, &fixture.service, &original, other));
            } else if comparison == "other-host" {
                let other = crate::service::test_fixture::ServiceFixture::new(
                    crate::multiplexer::state::MultiplexerState::new().0,
                );
                assert!(!witness.matches_original(&outcome, &other.service, &original, fork));
            } else {
                assert!(witness.matches_original(&outcome, &fixture.service, &original, fork));
            }
            assert!(matches!(
                outcome.into_result(),
                Err(RuntimeFailure::Superseded)
            ));
            assert_eq!(
                serde_json::to_value(fixture.service.mux.read().get_tab(&original.id).unwrap())
                    .unwrap(),
                replacement.lock().clone().unwrap()
            );
            assert!(for_tab(&original.id).is_none());
        }
    }
}

#[test]
fn removed_reused_and_unseeded_native_generations_never_become_fresh_rejections() {
    for history in [
        "existing",
        "reused",
        "acquired-later",
        "removed",
        "re-recorded",
        "revoked",
        "missing",
        "restored",
    ] {
        let (fixture, source, _peer, _cleanup) =
            runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
        let original = observed_target(&fixture, &source, history == "restored");
        let provider = original.agent_type.as_deref().unwrap();
        let nonce = original.session_binding_nonce.as_deref().unwrap();
        let installed = Arc::new(Mutex::new(None));
        if matches!(history, "existing" | "reused" | "removed" | "re-recorded") {
            let mut loaded = original.clone();
            loaded.agent_session_id = Some("fixture-original-session".into());
            *installed.lock() = Some(test_support::loaded_runtime(
                &loaded,
                fixture.service.agent_registry.provider_arc(provider),
            ));
            if history == "reused" {
                let existing = for_tab(&original.id).unwrap();
                bind(
                    &fixture.service,
                    &existing,
                    "fixture-original-session",
                    false,
                )
                .unwrap();
                let mut outcome = crate::async_runtime::block_on(ensure_expected_observed(
                    &fixture.service,
                    &original.id,
                    nonce,
                    None,
                ));
                assert!(outcome.take_rejection().is_none());
                assert!(Arc::ptr_eq(&existing, &outcome.into_result().unwrap()));
                test_support::stop_test(&original.id);
                continue;
            }
            if history != "existing" {
                assert!(crate::async_runtime::block_on(ensure_with_invocation(
                    &fixture.service,
                    &original.id,
                    None
                ))
                .is_err());
                test_support::stop_test(&original.id);
                assert!(for_tab(&original.id).is_none());
            }
            if history == "re-recorded" {
                crate::service::agent_runtime::authority::record_spawn(
                    &fixture.service,
                    &original.id,
                    provider,
                    nonce,
                )
                .unwrap();
            }
        } else if matches!(history, "revoked" | "missing") {
            crate::service::agent_runtime::authority::revoke(&original.id);
            if history == "revoked" {
                crate::service::agent_runtime::authority::record_spawn(
                    &fixture.service,
                    &original.id,
                    provider,
                    nonce,
                )
                .unwrap();
            }
        }
        let expected = original.clone();
        let retained = installed.clone();
        assert!(fixture
            .service
            .set_ext(ExpectedLaunchCommit {
                stage: "account_provision",
                callback: Mutex::new(Some(Box::new(move |svc| {
                    if history == "acquired-later" {
                        let mut loaded = expected.clone();
                        loaded.agent_session_id = Some("fixture-late-session".into());
                        *retained.lock() = Some(test_support::loaded_runtime(
                            &loaded,
                            svc.agent_registry
                                .provider_arc(loaded.agent_type.as_deref().unwrap()),
                        ));
                    } else {
                        svc.reserve_agent_launch_binding_nonce_if_current(&expected, None, None)
                            .unwrap();
                    }
                })))
            })
            .is_ok());
        let mut outcome = crate::async_runtime::block_on(ensure_expected_observed(
            &fixture.service,
            &original.id,
            nonce,
            None,
        ));
        assert!(outcome.take_rejection().is_none(), "{history}");
        assert!(outcome.into_result().is_err());
        test_support::stop_test(&original.id);
    }
}

#[tokio::test]
async fn observed_start_timeout_or_drop_keeps_retained_provisioning_unknown_and_never_continues() {
    use crate::service::provider_accounts::{provisioning, test_support as account_work};
    for cancel in [false, true] {
        let (fixture, source, _peer, _cleanup) =
            runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
        let original = observed_target(&fixture, &source, false);
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::sync_channel(1);
        let mut receipt = account_work::work_receipt(&fixture.service);
        assert!(fixture
            .service
            .set_ext(ExpectedLaunchCommit {
                stage: "account_provision",
                callback: Mutex::new(Some(Box::new(move |_| {
                    entered.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(10)).unwrap();
                })))
            })
            .is_ok());
        let mut pending = Box::pin(ensure_expected_observed(
            &fixture.service,
            &original.id,
            original.session_binding_nonce.as_deref().unwrap(),
            None,
        ));
        tokio::select! {
            _ = &mut pending => panic!("start ended before the account job entered"),
            entered = observed => entered.unwrap(),
        }
        assert_eq!(provisioning::retained_count(&fixture.service), 1);
        if cancel {
            drop(pending);
        } else {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(61)).await;
            let mut outcome = pending.await;
            assert!(outcome.take_rejection().is_none());
            assert!(outcome.into_result().is_err());
            tokio::time::resume();
        }
        let mut busy = ensure_expected_observed(
            &fixture.service,
            &original.id,
            original.session_binding_nonce.as_deref().unwrap(),
            None,
        )
        .await;
        assert!(busy.take_rejection().is_none());
        assert!(busy.into_result().is_err());
        assert_eq!(provisioning::retained_count(&fixture.service), 1);
        release.send(()).unwrap();
        account_work::settled(&mut receipt).await.unwrap();
        assert_eq!(provisioning::retained_count(&fixture.service), 0);
        assert!(for_tab(&original.id).is_none());
        assert!(!fixture
            .service
            .mux
            .read()
            .get_tab(&original.id)
            .unwrap()
            .launch_session_authenticated());
    }
}

#[test]
fn expected_generation_rejects_stale_normal_and_fork_entries_without_touching_replacement() {
    for fork in [None, Some(("source-thread", "source-turn"))] {
        let (fixture, id, _peer, _cleanup) =
            runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
        let original = fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .session_binding_nonce
            .clone()
            .unwrap();
        if fork.is_none() {
            let current = for_tab(&id).unwrap();
            let valid = crate::async_runtime::block_on(ensure_expected(
                &fixture.service,
                &id,
                &original,
                None,
            ))
            .unwrap();
            assert!(Arc::ptr_eq(&current, &valid));
        }
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .session_binding_nonce = Some("replacement-nonce".into());
        let before =
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        let session = for_tab(&id).unwrap();
        assert!(matches!(
            crate::async_runtime::block_on(ensure_expected(&fixture.service, &id, &original, fork)),
            Err(RuntimeFailure::Superseded)
        ));
        assert_eq!(
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(),
            before
        );
        assert!(Arc::ptr_eq(&session, &for_tab(&id).unwrap()));
    }
}

#[test]
fn original_generation_is_checked_at_restore_prompt_and_authority_commits() {
    for stage in [
        "account_provision",
        "restore_settings",
        "restore_materialize",
        "runtime_prompt",
        "record_spawn",
    ] {
        let (fixture, id, _peer, _cleanup) =
            runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
        let original = {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            if stage.starts_with("restore") {
                tab.restored_from_snapshot = true;
            }
            if stage == "restore_settings" {
                tab.agent_runtime_config = Some(serde_json::json!({"effort":"unsupported-effort"}));
            }
            tab.session_binding_nonce.clone().unwrap()
        };
        let provider = fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_type
            .clone()
            .unwrap();
        let saved = Arc::new(Mutex::new(None));
        let capture = saved.clone();
        let target = id.clone();
        let replacement_provider = provider.clone();
        assert!(fixture.service.set_ext(accounts::ExpectedLaunchCommit { stage, callback: Mutex::new(Some(Box::new(move |svc| {
            {
                let mut mux = svc.mux.write(); let current = mux.get_tab_mut(&target).unwrap();
                current.session_binding_nonce = Some("replacement-nonce".into());
                current.agent_runtime_config = Some(serde_json::json!({"systemPrompt":"replacement prompt","effort":"replacement-effort"}));
            }
            crate::service::agent_runtime::authority::record_spawn(svc, &target, &replacement_provider, "replacement-nonce").unwrap();
            *capture.lock() = Some(serde_json::to_value(svc.mux.read().get_tab(&target).unwrap()).unwrap());
        }))) }).is_ok());
        let result = match stage {
            "account_provision" => crate::async_runtime::block_on(ensure_expected(
                &fixture.service,
                &id,
                &original,
                None,
            ))
            .map(|_| ())
            .map_err(|error| {
                assert!(matches!(error, RuntimeFailure::Superseded));
                error.to_string()
            }),
            "record_spawn" => crate::service::agent_runtime::authority::record_spawn(
                &fixture.service,
                &id,
                &provider,
                &original,
            ),
            "runtime_prompt" => fixture
                .service
                .ensure_runtime_prompt_expected(&id, Some(&original)),
            _ => crate::service::agent_runtime::restore::tab_expected(
                &fixture.service,
                &id,
                Some(&original),
            )
            .map(|_| ()),
        };
        assert!(result.is_err(), "{stage}");
        assert_eq!(
            serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(),
            saved
                .lock()
                .clone()
                .expect("actual commit boundary reached")
        );
        assert!(crate::service::agent_runtime::authority::verifies(
            &fixture.service,
            &id,
            &provider,
            "replacement-nonce"
        ));
        assert!(!crate::service::agent_runtime::authority::verifies(
            &fixture.service,
            &id,
            &provider,
            &original
        ));
    }
}

#[test]
fn switch_launch_refuses_replacement_before_runtime_admission_and_bootstrap_write() {
    use crate::service::messenger::switch_handoff::{launch_authority, with_switch_launch};
    use std::{pin::Pin, task::Poll};
    use tokio::io::{AsyncRead, ReadBuf};
    for changed in ["team", "account", "namespace", "nonce"] {
        let (fixture, id, mut peer, _cleanup) =
            runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
        let session = for_tab(&id).unwrap();
        let expected = {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&id).unwrap();
            tab.agent_session_id = None;
            tab.reported_session_binding_nonce = None;
            if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
                &mut tab.backing
            {
                session_id.clear();
            }
            tab.clone()
        };
        with_switch_launch(&fixture.service, &expected, || {
            let authority = launch_authority(&fixture.service, &id).unwrap();
            let nonce = reserve_switch_launch(&fixture.service, &authority)?;
            session.context.lock().binding_nonce = nonce;
            session.context.lock().thread_id = None;
            {
                let mut mux = fixture.service.mux.write();
                let tab = mux.get_tab_mut(&id).unwrap();
                match changed {
                    "team" => tab.team_id = Some("replacement-team".into()),
                    "account" => {
                        tab.restorable_launch.account_id = Some("replacement-account".into())
                    }
                    "namespace" => {
                        tab.restorable_launch.account_namespace =
                            Some("replacement-namespace".into())
                    }
                    "nonce" => tab.session_binding_nonce = Some("replacement-nonce".into()),
                    _ => unreachable!(),
                }
            }
            let before =
                serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
            let params = crate::service::SpawnAgentParams {
                agent: expected.agent_type.clone(),
                skip_permissions: Some(false),
                ..Default::default()
            };
            assert!(fixture
                .service
                .prepare_agent_runtime_launch_options(&id, &params)
                .is_err());
            assert!(fixture
                .service
                .prepare_agent_runtime_launch_env(&id, &Default::default())
                .is_err());
            assert!(launch_switch_target(&fixture.service, &authority, Value::Null).is_err());
            let process = session.process.lock().clone().unwrap();
            crate::async_runtime::block_on(async {
                assert!(run(
                    &fixture.service,
                    &session.provider,
                    &session,
                    &process,
                    RuntimeOperation::Initialize
                )
                .await
                .is_err());
                std::future::poll_fn(|cx| {
                    let mut bytes = [0; 1];
                    let mut buffer = ReadBuf::new(&mut bytes);
                    assert!(Pin::new(&mut peer).poll_read(cx, &mut buffer).is_pending());
                    Poll::Ready(())
                })
                .await;
            });
            assert_eq!(
                serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(),
                before
            );
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn switch_launch_publication_cannot_clear_reparented_replacement() {
    use crate::service::messenger::switch_handoff::with_switch_launch;
    let (fixture, id, _peer, _cleanup) =
        runtime_fixture_without_remote_history(RUNTIME_FIXTURE_SOURCE);
    let session = for_tab(&id).unwrap();
    let expected = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let (entered, release) = super::super::publication::hold_publication_for_test(&id);
    let owner = fixture.service.clone();
    let target = id.clone();
    let publisher = std::thread::spawn(move || {
        with_switch_launch(&owner, &expected, || {
            let epoch = session.state.lock().epoch;
            session.state.lock().degrade(
                epoch,
                &RuntimeFailure::Refused("obsolete switch launch".into()),
            );
            publish(&owner, &target, &session);
            Ok(())
        })
    });
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let before = {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.team_id = Some("replacement-team".into());
        tab.parent_tab_id = Some("replacement-parent".into());
        tab.report_target_tab_id = Some("replacement-report".into());
        serde_json::to_value(tab).unwrap()
    };
    release.send(()).unwrap();
    publisher.join().unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn switch_handoff_conserves_the_claimed_payload_when_provider_proposes_a_replan() {
    use crate::service::messenger::{
        switch_handoff::SwitchSourceAuthority, AgentInput, MessageEnvelope, Messenger,
    };
    use std::{pin::Pin, task::Poll};
    use tokio::io::{AsyncRead, ReadBuf};
    for (completion, guarded) in [(false, true), (true, true), (true, false)] {
        let follow_up = if completion {
            "runtimeResponse(ctx, op, state, reply) { if (op.kind === 'submitTurn') return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:reply.value.turnId},notification:{method:'fixture/submit',params:op}};"
        } else {
            "runtimeResponse(ctx, op, state, reply) { if (op.kind === 'submitTurn') return {kind:'continue',beforeAcceptance:true,plan:{action:{kind:'request',call:{method:'fixture/submit',params:op}},state:null}};"
        };
        let source =
            RUNTIME_FIXTURE_SOURCE.replace("runtimeResponse(ctx, op, state, reply) {", follow_up);
        let (fixture, id, peer, _cleanup) = runtime_fixture_without_remote_history(&source);
        let session = for_tab(&id).unwrap();
        bind(&fixture.service, &session, "own-thread", false).unwrap();
        let ready = fixture.service.mux.read().get_tab(&id).unwrap().clone();
        let mut original = ready.clone();
        original.agent_type = Some("previous-provider".into());
        original.agent_session_id = Some("previous-session".into());
        let authority = SwitchSourceAuthority::admitted(&original)
            .ready_target(&ready)
            .unwrap();
        let input = AgentInput::Framed(
            Messenger::from(MessageEnvelope::from_codeterm())
                .framed_delivery("opaque switch handoff"),
        );
        let reader = crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(10), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "fixture/submit");
            assert_eq!(line.matches("opaque switch handoff").count(), 1);
            let error = if completion {
                serde_json::json!({"id":request["id"],"result":{"turnId":"opaque-turn"}})
            } else {
                serde_json::json!({"id":request["id"],"error":{"code":-1,"message":"opaque refusal"}})
            };
            peer.get_mut()
                .write_all(format!("{error}\n").as_bytes())
                .await
                .unwrap();
            if !guarded {
                let mut line = String::new();
                tokio::time::timeout(Duration::from_secs(10), peer.read_line(&mut line))
                    .await
                    .unwrap()
                    .unwrap();
                let notification: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(notification["method"], "fixture/submit");
                assert!(notification.get("id").is_none());
                assert_eq!(line.matches("opaque switch handoff").count(), 1);
            }
            peer.into_inner()
        });
        if guarded {
            assert!(fixture
                .service
                .send_provider_switch_handoff(authority.clone(), &input)
                .is_err());
            assert!(authority.was_claimed());
            assert!(fixture
                .service
                .send_provider_switch_handoff(authority, &input)
                .is_err());
        } else {
            crate::service::agent_runtime::transport::send(&fixture.service, &id, &input, None)
                .unwrap();
            assert!(!authority.was_claimed());
        }
        crate::async_runtime::block_on(async {
            let mut peer = reader.await.unwrap();
            std::future::poll_fn(|cx| {
                let mut bytes = [0; 1];
                let mut buffer = ReadBuf::new(&mut bytes);
                assert!(Pin::new(&mut peer).poll_read(cx, &mut buffer).is_pending());
                Poll::Ready(())
            })
            .await;
        });
    }
}

#[test]
fn fresh_runtime_history_is_empty_before_rollout_and_readable_when_ready() {
    assert_fresh_history_ready(RuntimeAuthState::SignedIn, true);
}

#[test]
fn fresh_runtime_without_auth_evidence_is_ready_before_history() {
    assert_fresh_history_ready(RuntimeAuthState::Unknown, true);
}

#[test]
fn undeclared_remote_history_keeps_runtime_ready_without_read_history() {
    assert_fresh_history_ready(RuntimeAuthState::Unknown, false);
}

fn assert_fresh_history_ready(auth: RuntimeAuthState, remote_history: bool) {
    let source = RUNTIME_FIXTURE_SOURCE
        .replace("module.exports = {", "module.exports = { runtimeStoredHistoryAvailable: () => false,")
        .replace("resumeThread:'fixture/resume'", "openThread:'fixture/open',resumeThread:'fixture/resume'")
        .replace("if (op.kind === 'resumeThread')", "if (op.kind === 'openThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:'own-thread',settings:op.settings}}; if (op.kind === 'resumeThread')");
    let (fixture, tab_id, peer, _cleanup) = if remote_history {
        runtime_fixture_with_stored_history(&source)
    } else {
        runtime_fixture_without_remote_history(&source)
    };
    let _storage = fixture.install_seeded_storage_desk();
    let session = for_tab(&tab_id).unwrap();
    *session.auth.lock() = auth;
    session
        .thread_loaded
        .store(false, std::sync::atomic::Ordering::Release);
    session.state.lock().phase = RuntimePhase::Loading;
    session.context.lock().thread_id = None;
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.agent_booting = true;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
        if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
            &mut tab.backing
        {
            session_id.clear();
        }
    }
    let owner = fixture.service.clone();
    let target = tab_id.clone();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let methods = if remote_history {
            vec!["fixture/initialize", "fixture/open", "fixture/history"]
        } else {
            vec!["fixture/initialize", "fixture/open"]
        };
        for method in methods {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            if method == "fixture/history" {
                let tab = owner.mux.read().get_tab(&target).unwrap().clone();
                assert!(!tab.agent_booting);
                assert_eq!(
                    tab.boot_stage,
                    Some(codeterm_types::pty::BootStage::AgentReady)
                );
                let transcript =
                    crate::service::tab_chat::tab_chat_snapshot(&owner, &target, 0).unwrap();
                assert_eq!(transcript, (Vec::new(), 0));
                for config in [
                    serde_json::json!({}),
                    serde_json::json!({"unusedThreadId":"foreign-thread"}),
                ] {
                    owner
                        .mux
                        .write()
                        .get_tab_mut(&target)
                        .unwrap()
                        .agent_runtime_config = Some(config);
                    let error = crate::service::tab_chat::tab_chat_snapshot(&owner, &target, 0)
                        .unwrap_err();
                    assert_eq!(
                        crate::service::tab_chat::runtime_stored::StoredHistoryFailure::from_error(
                            &error
                        ),
                        Some(
                            crate::service::tab_chat::runtime_stored::StoredHistoryFailure::Missing
                        )
                    );
                }
                owner
                    .mux
                    .write()
                    .get_tab_mut(&target)
                    .unwrap()
                    .agent_runtime_config =
                    Some(serde_json::json!({"unusedThreadId":"own-thread"}));
            }
            peer.get_mut()
                .write_all(
                    format!(
                        "{}\n",
                        serde_json::json!({
                            "jsonrpc":"2.0","id":request["id"],"result":{}
                        })
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
        peer
    });
    let _peer = crate::async_runtime::block_on(async {
        let result = ensure_locked(
            &fixture.service,
            &tab_id,
            &session.provider,
            &serde_json::json!({}),
            &session,
            None,
        )
        .await;
        if result.is_err() {
            peer.abort();
        }
        let peer_result = peer.await;
        result.unwrap();
        peer_result.unwrap()
    });
    if !remote_history {
        assert_eq!(session.state.lock().phase, RuntimePhase::Ready);
        assert!(session.history_retry.lock().is_none());
        assert_eq!(
            session.cache.lock().backfill(),
            model::HistoryBackfill::Complete
        );
    }
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert!(!tab.agent_booting);
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::AgentReady)
    );
    assert!(session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap(),
        (Vec::new(), 0)
    );
}

#[test]
fn local_resume_completes_without_rpc_and_refuses_foreign_or_retired_binding() {
    use std::{pin::Pin, task::Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    let source = RUNTIME_FIXTURE_SOURCE.replace(
        "const method =",
        "if (op.kind === 'resumeThread') return {action:{kind:'local'},state:null}; const method =",
    );
    let (fixture, tab_id, mut peer, _cleanup) = super::sync_send::runtime_fixture_with(&source);
    let session = for_tab(&tab_id).unwrap();
    let process = session.process.lock().clone().unwrap();
    let resume = RuntimeOperation::ResumeThread {
        thread_id: "own-thread".into(),
        instructions: String::new(),
        settings: Default::default(),
    };
    crate::async_runtime::block_on(async {
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run(
                &fixture.service,
                &session.provider,
                &session,
                &process,
                resume.clone(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            matches!(result, RuntimeResult::ThreadLoaded { thread_id, .. } if thread_id == "own-thread")
        );
        let foreign = RuntimeOperation::ResumeThread {
            thread_id: "foreign".into(),
            instructions: String::new(),
            settings: Default::default(),
        };
        assert!(matches!(
            run(
                &fixture.service,
                &session.provider,
                &session,
                &process,
                foreign
            )
            .await,
            Err(RuntimeFailure::Superseded)
        ));
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some("retired".into());
        assert!(matches!(
            run(
                &fixture.service,
                &session.provider,
                &session,
                &process,
                resume
            )
            .await,
            Err(RuntimeFailure::Superseded)
        ));
        std::future::poll_fn(|cx| {
            let mut bytes = [0; 1];
            let mut buffer = ReadBuf::new(&mut bytes);
            assert!(Pin::new(&mut peer).poll_read(cx, &mut buffer).is_pending());
            Poll::Ready(())
        })
        .await;
    });
}
