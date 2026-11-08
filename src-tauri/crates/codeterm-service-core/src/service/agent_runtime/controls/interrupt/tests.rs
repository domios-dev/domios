use super::*;
use codeterm_plugin::agent_runtime::{RuntimeActiveTurn, RuntimeThreadParent};
use std::future::Future;

fn context() -> RuntimeContext {
    RuntimeContext {
        restricted_cli: None,
        host_owned: Default::default(),
        epoch: 7,
        binding_nonce: "nonce".into(),
        thread_id: Some("root".into()),
        active_turn_id: Some("root-turn".into()),
        active_turns: vec![RuntimeActiveTurn {
            thread_id: "child".into(),
            turn_id: "child-turn".into(),
            parent_thread_id: Some("root".into()),
        }],
        thread_parents: vec![RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "root".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn captured_root_and_descendants_require_every_exact_completion() {
    let context = context();
    let target = InterruptTarget::capture(&context).unwrap();
    assert_eq!(target.turns.len(), 2);
    assert_eq!(
        target.state(&context, &RuntimePhase::Ready, |_, _| false),
        CompletionState::Waiting
    );
    assert_eq!(
        target.state(&context, &RuntimePhase::Ready, |thread, _| thread == "root"),
        CompletionState::Waiting
    );
    assert_eq!(
        target.state(&context, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Waiting
    );
    let mut retired = context.clone();
    retired.active_turn_id = None;
    assert_eq!(
        target.state(&retired, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Waiting
    );
    retired.active_turns.clear();
    assert_eq!(
        target.state(&retired, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Complete
    );
    retired.active_turn_id = Some("next-root-turn".into());
    retired.active_turns.push(RuntimeActiveTurn {
        thread_id: "child".into(),
        turn_id: "next-child-turn".into(),
        parent_thread_id: Some("root".into()),
    });
    assert_eq!(
        target.state(&retired, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Complete
    );
}

#[test]
fn stop_without_a_live_turn_clears_stale_working_state_and_returns_not_working() {
    let (fixture, id) = super::super::tests::runtime_fixture();
    crate::async_runtime::block_on(async {
        let session = engine::ensure(&fixture.service, &id).await.unwrap();
        session.state.lock().working(false, 1, None);
        engine::publish(&fixture.service, &id, &session);
        let live = crate::agent::state_verdict::live_state_for_runtime(
            &codeterm_plugin::agent_runtime::RuntimeTurnState::Working,
            None,
            Some("stale command"),
        );
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .agent_state = Some(live);
        assert!(session.context.lock().active_turn_id.is_none());
        assert!(session.context.lock().active_turns.is_empty());
        let result =
            crate::service::stop_execution::stop_agent_turn(fixture.service.clone(), id.clone())
                .await
                .unwrap();
        assert_eq!(
            result,
            serde_json::json!({"ok":true,"stopped":false,"reason":"not_working","status":"idle","route":"runtime"})
        );
        assert!(session.state.lock().active_turn_started_at_ms.is_none());
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(
            tab.agent_state.as_ref().unwrap().state,
            codeterm_types::pty::TimelineState::Idle
        );
        assert!(tab.agent_state.as_ref().unwrap().status_line.is_none());
        assert!(tab.agent_runtime_state.as_ref().unwrap()["active_turn_started_at_ms"].is_null());
    });
}

#[test]
fn captured_current_root_turn_replaces_a_stale_roster_entry() {
    let mut context = context();
    context.active_turns.push(RuntimeActiveTurn {
        thread_id: "root".into(),
        turn_id: "stale-root-turn".into(),
        parent_thread_id: None,
    });
    let target = InterruptTarget::capture(&context).unwrap();
    assert_eq!(
        target
            .turns
            .iter()
            .filter(|turn| turn.thread_id == "root")
            .map(|turn| turn.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["root-turn"]
    );
}

#[test]
fn superseded_epoch_and_unload_never_report_completion() {
    let context = context();
    let target = InterruptTarget::capture(&context).unwrap();
    let mut newer = context.clone();
    newer.epoch += 1;
    assert_eq!(
        target.state(&newer, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Superseded
    );
    newer = context.clone();
    newer.binding_nonce = "other".into();
    assert_eq!(
        target.state(&newer, &RuntimePhase::Ready, |_, _| true),
        CompletionState::Superseded
    );
    assert_eq!(
        target.state(
            &context,
            &RuntimePhase::Unloaded { reason: None },
            |_, _| true
        ),
        CompletionState::Unloaded
    );
}

#[test]
fn mock_runtime_interrupt_waits_for_events_and_deadline_degrades_without_idle() {
    let (fixture, id) = super::super::tests::runtime_fixture();
    fixture
        .service
        .agent_switch_model_with_options(&id, "target", None, false)
        .unwrap();
    let session = engine::for_tab(&id).unwrap();
    let process = session.process.lock().clone().unwrap();
    crate::async_runtime::block_on(async {
        let mut context = session.context.lock().clone();
        context.active_turn_id = Some("root-turn".into());
        context.active_turns.push(RuntimeActiveTurn {
            thread_id: "child".into(),
            turn_id: "child-turn".into(),
            parent_thread_id: Some("own-thread".into()),
        });
        context.thread_parents.push(RuntimeThreadParent {
            thread_id: "child".into(),
            parent_thread_id: "own-thread".into(),
        });
        *session.context.lock() = context.clone();
        let target = InterruptTarget::capture(&context).unwrap();
        let mut changes = session.control_changes.subscribe();
        let mut ended = process.ended.clone();
        let waiting = wait(
            &fixture.service,
            &session,
            &target,
            &mut changes,
            &mut ended,
            Duration::from_secs(1),
        );
        tokio::pin!(waiting);
        std::future::poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        engine::record_completed_turn(&session, "own-thread", Some("root-turn"));
        std::future::poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        engine::record_completed_turn(&session, "child", Some("child-turn"));
        std::future::poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        session.context.lock().active_turn_id = None;
        session.control_changes.send_replace(());
        std::future::poll_fn(|cx| {
            assert!(waiting.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        session.context.lock().active_turns.clear();
        session.control_changes.send_replace(());
        waiting.await.unwrap();
    });
    {
        let mut context = session.context.lock();
        context.active_turn_id = Some("reply-turn".into());
        context
            .active_turns
            .retain(|turn| turn.thread_id != "own-thread");
        context.active_turns.push(RuntimeActiveTurn {
            thread_id: "own-thread".into(),
            turn_id: "reply-turn".into(),
            parent_thread_id: None,
        });
    }
    super::interrupt(&fixture.service, &id).unwrap();
    assert!(session
        .completions
        .lock()
        .contains("own-thread", "reply-turn"));
    crate::async_runtime::block_on(async {
        session.context.lock().active_turn_id = Some("never-completes".into());
        let target = InterruptTarget::capture(&session.context.lock()).unwrap();
        let mut changes = session.control_changes.subscribe();
        let mut ended = process.ended.clone();
        session.context.lock().epoch += 1;
        assert!(matches!(
            wait(
                &fixture.service,
                &session,
                &target,
                &mut changes,
                &mut ended,
                Duration::from_secs(1)
            )
            .await,
            Err(RuntimeFailure::Superseded)
        ));
        session.context.lock().epoch = target.epoch;
        session.state.lock().phase = RuntimePhase::Unloaded {
            reason: Some("fixture unload".into()),
        };
        assert!(wait(
            &fixture.service,
            &session,
            &target,
            &mut changes,
            &mut ended,
            Duration::from_secs(1)
        )
        .await
        .is_err());
        session.state.lock().phase = RuntimePhase::Ready;
        let pending = fixture
            .service
            .chat_send_ledger
            .try_start(&id, "opaque uncertain input", None)
            .unwrap();
        assert!(fixture.service.chat_send_ledger.bind_rpc_send(
            &pending.send_id,
            crate::service::chat_send_transaction::SendLiveBinding {
                session_id: Some("own-thread".into()),
                binding_nonce: Some("own-nonce".into())
            }
        ));
        fixture
            .service
            .chat_send_ledger
            .mark_rpc_uncertain(&pending.send_id);
        let prior = fixture
            .service
            .chat_send_ledger
            .status(&pending.send_id)
            .unwrap()
            .state;
        assert!(wait(
            &fixture.service,
            &session,
            &target,
            &mut changes,
            &mut ended,
            Duration::ZERO
        )
        .await
        .is_err());
        assert_eq!(
            fixture
                .service
                .chat_send_ledger
                .status(&pending.send_id)
                .unwrap()
                .state,
            prior
        );
        assert!(matches!(
            session.state.lock().phase,
            RuntimePhase::Degraded { .. }
        ));
        assert!(session.context.lock().active_turn_id.is_none());
    });
    engine::stop_blocking(&fixture.service, &id).unwrap();
}

#[test]
fn stop_with_no_provider_turn_settles_at_once_without_interrupt_or_degrade() {
    use crate::service::agent_runtime::{
        engine::test_support::runtime_fixture_turn_scoped, turn_requests::tests as turn_fixture,
    };
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::{AsyncBufRead, BufReader};
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(turn_fixture::SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let (thread, epoch) = {
        let context = session.context.lock();
        (context.thread_id.clone().unwrap(), context.epoch)
    };
    crate::async_runtime::block_on(async {
        super::super::super::subscriber::publish_events(
            &fixture.service,
            &session,
            epoch,
            vec![
                codeterm_plugin::agent_runtime::AgentRuntimeEvent::TurnState {
                    thread_id: thread,
                    turn_id: Some("no-prompt-behind-this".into()),
                    parent_thread_id: None,
                    state: codeterm_plugin::agent_runtime::RuntimeTurnState::Working,
                    detail: None,
                    status_line: None,
                    started_at: None,
                    completed_at: None,
                },
            ],
            1,
        )
        .await
        .unwrap();
        assert!(session.context.lock().active_turn_id.is_some());
        let result =
            crate::service::stop_execution::stop_agent_turn(fixture.service.clone(), tab.clone())
                .await
                .unwrap();
        assert_eq!(
            result,
            serde_json::json!({"ok":true,"stopped":false,"reason":"not_working","status":"idle","route":"runtime"})
        );
    });
    assert!(session.context.lock().active_turn_id.is_none());
    assert!(session.state.lock().active_turn_started_at_ms.is_none());
    assert!(matches!(session.state.lock().phase, RuntimePhase::Ready));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .agent_state
            .as_ref()
            .unwrap()
            .state,
        codeterm_types::pty::TimelineState::Idle
    );
    let mut peer = BufReader::new(peer);
    assert!(
        matches!(
            Pin::new(&mut peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ),
        "no interrupt reaches a provider that has no turn"
    );
}

#[test]
fn before_first_update_interrupt_requires_declaration_and_exact_outstanding_submit() {
    for (declared, outstanding, invoked) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let (fixture, id) = super::super::tests::runtime_fixture_with_interrupt(declared);
        crate::async_runtime::block_on(async {
            let session = engine::ensure(&fixture.service, &id).await.unwrap();
            let context = session.context.lock().clone();
            assert!(context.active_turn_id.is_none());
            session.context.lock().provider_settings["interruptProbe"] = serde_json::json!(true);
            if outstanding {
                let status = fixture
                    .service
                    .chat_send_ledger
                    .try_start_scoped_with_normalization(
                        &id,
                        "opaque",
                        None,
                        None,
                        Default::default(),
                    )
                    .unwrap();
                assert!(fixture.service.chat_send_ledger.bind_rpc_send(
                    &status.send_id,
                    crate::service::chat_send_transaction::SendLiveBinding {
                        session_id: context.thread_id.clone(),
                        binding_nonce: Some(context.binding_nonce.clone())
                    }
                ));
                session.context.lock().claimed_sends.push(
                    codeterm_plugin::agent_runtime::ClaimedSend {
                        send_id: status.send_id,
                        thread_id: context.thread_id.unwrap(),
                        input: codeterm_plugin::agent_runtime::TurnInput {
                            text: "opaque".into(),
                            attachments: Vec::new(),
                        },
                    },
                );
            }
            let captured = session.context.lock().clone();
            assert!(!before_first_turn(
                &fixture.service,
                &session,
                &RuntimeContext {
                    epoch: captured.epoch + 1,
                    ..captured.clone()
                }
            ));
            assert!(!before_first_turn(
                &fixture.service,
                &session,
                &RuntimeContext {
                    binding_nonce: "foreign".into(),
                    ..captured.clone()
                }
            ));
            assert!(!before_first_turn(
                &fixture.service,
                &session,
                &RuntimeContext {
                    thread_id: Some("foreign".into()),
                    ..captured.clone()
                }
            ));
            assert_eq!(
                before_first_turn(&fixture.service, &session, &captured),
                declared && outstanding
            );
            let result = crate::service::stop_execution::stop_agent_turn(
                fixture.service.clone(),
                id.clone(),
            )
            .await
            .unwrap();
            assert_eq!(result["stopped"], serde_json::json!(invoked));
            assert_eq!(
                session.context.lock().settings.effort.as_deref() == Some("high"),
                invoked
            );
        });
    }
}

#[tokio::test]
async fn declared_cancel_without_reply_settles_interrupted_after_one_grace() {
    use crate::service::agent_runtime::{
        engine::test_support::runtime_fixture_turn_scoped, turn_requests::tests as turn_fixture,
    };
    use tokio::io::{AsyncBufReadExt, BufReader};
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(turn_fixture::SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let operation = turn_fixture::operation(&session.context.lock());
    let (accepted, receipt) = tokio::sync::oneshot::channel();
    let submit = async {
        let (result, _) = engine::run_submit(&fixture.service, &session, &process, operation).await;
        assert!(matches!(
            result,
            Ok(RuntimeResult::TurnAccepted { .. }) | Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
        accepted.send(()).unwrap();
    };
    let server = async {
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        turn_fixture::accept(&fixture.service, &session).await;
        receipt.await.unwrap();
        let context = session.context.lock().clone();
        let target = InterruptTarget::capture(&context).unwrap();
        let mut changes = session.control_changes.subscribe();
        let mut ended = process.ended.clone();
        let result = engine::run(
            &fixture.service,
            &session.provider,
            &session,
            &process,
            RuntimeOperation::Interrupt {
                thread_id: context.thread_id.clone().unwrap(),
                active_turn_id: context.active_turn_id.clone(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(result, RuntimeResult::Done));
        line.clear();
        peer.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["method"],
            "fixture/cancel"
        );
        tokio::time::pause();
        let (result, ()) = tokio::join!(biased;
            wait(&fixture.service,&session,&target,&mut changes,&mut ended,Duration::from_secs(30)),
            async {tokio::time::advance(Duration::from_secs(31)).await;},
        );
        assert!(result.is_ok());
        assert!(session.context.lock().active_turn_id.is_none());
        assert!(session
            .completions
            .lock()
            .contains(context.thread_id.as_deref().unwrap(), "turn"));
        assert!(session
            .cache
            .lock()
            .snapshot()
            .iter()
            .any(|row| row.content.contains("cancel grace expired")));
        peer
    };
    let ((), _peer) = tokio::join!(submit, server);
}
