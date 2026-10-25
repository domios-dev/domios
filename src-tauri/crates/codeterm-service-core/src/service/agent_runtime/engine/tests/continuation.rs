use super::super::*;
use super::sync_send::runtime_fixture;
use crate::multiplexer::{state::MultiplexerState, types::request_continue};
use crate::service::{
    chat_send_transaction::ChatSendState,
    messenger::{MessageEnvelope, Messenger},
    turn_resume::{self, Step, Wave},
};
use serde_json::json;
use std::sync::atomic::Ordering;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn non_runtime_chat_continuation_is_refused_and_skipped_without_delivery() {
    for execution_mode in [false, true] {
        let (fixture, tab_id, mut peer, _cleanup) = runtime_fixture();
        if execution_mode {
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .restorable_launch
                .execution_mode = Some("opaque-cloud".into());
        } else {
            fixture.service.agent_registry.register_provider(Box::new(
                crate::agent::test_provider::MockProvider::new("sync-send-fixture"),
            ));
        }
        assert!(!super::super::super::requested_for_tab(
            &fixture.service,
            &tab_id
        ));
        assert_eq!(
            fixture
                .service
                .request_turn_resume_for_caller(&tab_id, &tab_id, "opaque"),
            Err(turn_resume::NO_AGENT_SESSION.into())
        );
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            assert!(tab.turn_resume.is_none());
            request_continue(
                &mut tab.turn_resume,
                "own-thread",
                "before",
                "opaque".into(),
            );
        }
        assert_eq!(
            turn_resume::step_for_service(&fixture.service, &tab_id, "after"),
            Step::Skipped(turn_resume::NO_AGENT_SESSION)
        );
        {
            let mut mux = fixture.service.mux.write();
            *mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
            assert_eq!(
                mux.get_tab(&tab_id)
                    .unwrap()
                    .turn_resume
                    .as_ref()
                    .unwrap()
                    .settled
                    .as_ref()
                    .unwrap()
                    .outcome,
                crate::multiplexer::types::TurnResumeOutcome::Skipped(
                    turn_resume::NO_AGENT_SESSION.into()
                )
            );
        }
        assert_eq!(
            turn_resume::step_for_service(&fixture.service, &tab_id, "third"),
            Step::Settled
        );
        assert!(fixture
            .ui
            .0
            .lock()
            .iter()
            .all(|(event, _)| event != "turn-resume-incident" && event != "chat-send-status"));
        assert_no_rpc(&mut peer);
    }
}

#[test]
fn loaded_thread_trigger_delivers_restart_continuation_once_through_real_routing() {
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture();
    let session = for_tab(&tab_id).unwrap();
    let nonce = session.context.lock().binding_nonce.clone();
    {
        let mut mux = fixture.service.mux.write();
        request_continue(
            &mut mux.get_tab_mut(&tab_id).unwrap().turn_resume,
            "own-thread",
            "before-restart",
            "opaque continuation body".into(),
        );
        *mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
        mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some(nonce.clone());
        assert!(mux.get_tab(&tab_id).unwrap().pty_id.is_none());
    }
    let storage = fixture.install_seeded_storage_desk();
    let persisted = crate::session::store::StorageDeskAdapter::from_storage(storage);
    let wave = Arc::new(Wave::new(
        "after-restart",
        turn_resume::owed_tab_ids(&fixture.service.mux, "after-restart"),
    ));
    assert!(fixture.service.set_ext(wave.clone()).is_ok());
    session.state.lock().phase = RuntimePhase::Loading;
    session.thread_loaded.store(false, Ordering::Release);
    let (written_tx, written_rx) = std::sync::mpsc::channel();
    let target = tab_id.clone();
    fixture.ui.listen("chat-send-status", move |value| {
        if value["tab_id"] == target && value["state"] == "written" {
            let _ = written_tx.send(());
        }
    });
    let owner = fixture.service.clone();
    let target = tab_id.clone();
    let runtime = session.clone();
    let pending_wave = wave.clone();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        for method in [
            "fixture/initialize",
            "fixture/resume",
            "fixture/history",
            "fixture/submit",
        ] {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            if method == "fixture/history" {
                assert_eq!(
                    owner.mux.read().get_tab(&target).unwrap().boot_stage,
                    Some(codeterm_types::pty::BootStage::AgentReady)
                );
                assert!(!runtime.thread_loaded.load(Ordering::Acquire));
                owner.on_turn_resume_binding_changed(&target);
                assert!(pending_wave.is_pending(&target));
                assert!(owner
                    .mux
                    .read()
                    .get_tab(&target)
                    .unwrap()
                    .turn_resume
                    .as_ref()
                    .unwrap()
                    .settled
                    .is_none());
                assert_no_buffered_rpc(&mut peer);
            }
            if method == "fixture/submit" {
                assert!(runtime.thread_loaded.load(Ordering::Acquire));
                assert!(!pending_wave.is_pending(&target));
                assert_eq!(request["params"]["threadId"], "own-thread");
                assert_eq!(
                    request["params"]["input"]["text"],
                    Messenger::from(MessageEnvelope::codeterm_reminder("turn-resume"))
                        .framed_delivery("opaque continuation body")
                        .to_message()
                );
            }
            let response = json!({"id":request["id"],"result":{"turnId":"accepted-turn"}});
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
            if method == "fixture/submit" {
                return (
                    peer,
                    request["params"]["sendId"].as_str().unwrap().to_owned(),
                );
            }
        }
        unreachable!()
    });
    crate::async_runtime::block_on(ensure_locked(
        &fixture.service,
        &tab_id,
        "sync-send-fixture",
        &json!({}),
        &session,
        None,
    ))
    .unwrap();
    let (mut peer, send_id) = crate::async_runtime::block_on(peer).unwrap();
    written_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let ledger = &fixture.service.chat_send_ledger;
    assert_eq!(
        ledger.status(&send_id).unwrap().state,
        ChatSendState::Written
    );
    assert!(ledger
        .confirm_rpc_send(&tab_id, "foreign-thread", &nonce, &send_id, Some("item"))
        .is_none());
    assert!(ledger
        .confirm_rpc_send(
            &tab_id,
            "own-thread",
            "foreign-nonce",
            &send_id,
            Some("item")
        )
        .is_none());
    assert!(ledger
        .confirm_rpc_send(&tab_id, "own-thread", &nonce, &send_id, Some("item"))
        .is_some());
    assert_eq!(ledger.status(&send_id).unwrap().state, ChatSendState::Sent);
    fixture.service.on_turn_resume_binding_changed(&tab_id);
    assert_eq!(
        turn_resume::step_for_service(&fixture.service, &tab_id, "after-restart"),
        Step::Settled
    );
    {
        let mut mux = fixture.service.mux.write();
        *mux = MultiplexerState::from_session_snapshot(&persisted.snapshot().unwrap());
        assert_eq!(
            mux.get_tab(&tab_id)
                .unwrap()
                .turn_resume
                .as_ref()
                .unwrap()
                .settled
                .as_ref()
                .unwrap()
                .outcome,
            crate::multiplexer::types::TurnResumeOutcome::Sent
        );
    }
    assert_eq!(
        turn_resume::step_for_service(&fixture.service, &tab_id, "third-start"),
        Step::Settled
    );
    assert!(fixture
        .ui
        .0
        .lock()
        .iter()
        .all(|(event, _)| event != "turn-resume-incident"));
    assert_no_buffered_rpc(&mut peer);
}

fn assert_no_buffered_rpc(peer: &mut BufReader<tokio::io::DuplexStream>) {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufRead;
    assert!(matches!(
        Pin::new(peer).poll_fill_buf(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

fn assert_no_rpc(peer: &mut tokio::io::DuplexStream) {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::{AsyncRead, ReadBuf};
    let mut bytes = [0; 1];
    assert!(matches!(
        Pin::new(peer).poll_read(
            &mut Context::from_waker(Waker::noop()),
            &mut ReadBuf::new(&mut bytes)
        ),
        Poll::Pending
    ));
}
