use super::*;
use codeterm_plugin::agent_runtime::{RuntimeActiveTurn, RuntimeThreadParent};
use std::future::Future;

fn context() -> RuntimeContext {
    RuntimeContext {
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
        CompletionState::Complete
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
    tauri::async_runtime::block_on(async {
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
        waiting.await.unwrap();
    });
    session.context.lock().active_turn_id = Some("reply-turn".into());
    super::interrupt(&fixture.service, &id).unwrap();
    assert!(session
        .completions
        .lock()
        .contains("own-thread", "reply-turn"));
    tauri::async_runtime::block_on(async {
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
        assert_eq!(
            session.context.lock().active_turn_id.as_deref(),
            Some("never-completes")
        );
    });
    engine::stop(&id);
}
