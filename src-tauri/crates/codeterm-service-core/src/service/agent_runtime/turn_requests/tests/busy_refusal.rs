use super::*;

#[tokio::test]
async fn busy_rpc_refusal_of_a_written_prompt_stays_uncertain_until_its_late_echo_delivers_it() {
    use crate::service::chat_send_transaction::{ChatSendState, SendLiveBinding};
    use crate::service::pending_delivery_payloads::user_sends::{
        continuity::ReceiptState, UserSendAuthority,
    };
    let source = SOURCE
        .replace("turnId:'turn'", "turnId:op.sendId")
        .replace("beforeAcceptance:false", "beforeAcceptance:true");
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(&source);
    let session = engine::for_tab(&tab).unwrap();
    let process = session.process.lock().clone().unwrap();
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let server = async move {
        let mut peer = BufReader::new(peer);
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            requests.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        let first = &requests[0];
        let second = &requests[1];
        let reply = json!({"id":second["id"],"error":{"code":-32602,"message":"opaque"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        wait.await.unwrap();
        let reply = json!({"id":first["id"],"result":{"stopReason":"end_turn"}});
        peer.get_mut()
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .unwrap();
        peer
    };
    let exercise = async {
        let mut first = operation(&session.context.lock());
        if let RuntimeOperation::SubmitTurn { send_id, .. } = &mut first {
            *send_id = "a".into();
        }
        let (_, written) = engine::run_submit(&fixture.service, &session, &process, first).await;
        assert!(written);
        let context = session.context.lock().clone();
        let store = &fixture.service.pending_delivery_payloads.user_sends;
        let ledger = &fixture.service.chat_send_ledger;
        // Every service user-send reservation binds the ledger's owner; startup binds it too.
        fixture.service.bind_user_send_owner();
        let reservation = store
            .reserve(
                UserSendAuthority {
                    tab_id: tab.clone(),
                    pty_id: None,
                    binding_nonce: context.binding_nonce.clone(),
                    pty_session_generation: None,
                    provider_id: session.provider.clone(),
                    session_id: context.thread_id.clone(),
                    observed_session_id: context.thread_id.clone(),
                    prelaunch: false,
                },
                None,
                || ledger.reserve_identity(),
            )
            .unwrap();
        let record = store.admit(&reservation, "opaque", "opaque", None).unwrap();
        drop(reservation);
        ledger.admit_user(&record);
        // The same handoff order as the runtime user drain: claim, ledger handoff, RPC binding.
        assert!(store.claim(&record, context.thread_id.as_deref()).unwrap());
        assert!(ledger.begin_user_handoff(
            &record,
            "opaque",
            fixture.service.chat_send_anchor(&tab)
        ));
        assert!(ledger.bind_rpc_send(
            &record.send_id,
            SendLiveBinding {
                session_id: context.thread_id.clone(),
                binding_nonce: Some(context.binding_nonce.clone()),
            }
        ));
        session.context.lock().claimed_sends.push(ClaimedSend {
            send_id: record.send_id.clone(),
            thread_id: context.thread_id.clone().unwrap(),
            input: TurnInput {
                text: "opaque".into(),
                attachments: vec![],
            },
        });
        let mut second = operation(&context);
        if let RuntimeOperation::SubmitTurn {
            send_id,
            active_turn_id,
            ..
        } = &mut second
        {
            *send_id = record.send_id.clone();
            *active_turn_id = context.active_turn_id.clone();
        }
        let mut changes = session.control_changes.subscribe();
        let (_, written) = engine::run_submit(&fixture.service, &session, &process, second).await;
        assert!(written);
        loop {
            changes.borrow_and_update();
            if !session.turn_requests.lock().contains_key(&record.send_id) {
                break;
            }
            changes.changed().await.unwrap();
        }
        // A refusal of a written prompt is not proof of non-delivery: no failure, no Retry,
        // but the unconfirmed bubble says why it is not pending forever.
        let refused = ledger.status(&record.send_id).unwrap();
        assert_eq!(refused.state, ChatSendState::Unknown);
        let reason = |status: &crate::service::chat_send_transaction::ChatSendStatus| {
            status.error.as_deref().is_some_and(|error| {
                error.starts_with(crate::service::chat_send_transaction::RPC_REFUSED_WHILE_BUSY)
            })
        };
        assert!(reason(&refused), "{:?}", refused.error);
        assert_eq!(session.context.lock().active_turn_id.as_deref(), Some("a"));
        assert!(store
            .receipts(&tab)
            .unwrap()
            .iter()
            .all(|receipt| receipt.state != ReceiptState::Failed));
        // The runtime user drain's own settlement after a written handoff, in its order.
        store.finish_handoff(&record, false).unwrap();
        let finished = ledger
            .finish(&record.send_id, Err("submit_unconfirmed: opaque".into()))
            .unwrap();
        assert_eq!(finished.state, ChatSendState::Unknown);
        assert!(
            reason(&finished),
            "late transport classification keeps the reason"
        );
        assert!(store.complete(&record).unwrap());
        let settled = ledger.status(&record.send_id).unwrap();
        store.persist_runtime_status(&settled).unwrap();
        crate::service::agent_runtime::envelope_delivery::record_write(&fixture.service, &settled)
            .unwrap();
        // The busy agent finishes its turn, then runs the prompt it reported as refused.
        release.send(()).unwrap();
        loop {
            changes.borrow_and_update();
            if session.completions.lock().contains("own-thread", "a") {
                break;
            }
            changes.changed().await.unwrap();
        }
        assert!(session.context.lock().active_turn_id.is_none());
        let thread = context.thread_id.clone().unwrap();
        // Every precondition of the late confirmation, checked separately so a failure names it.
        assert!(
            session
                .context
                .lock()
                .claimed_sends
                .iter()
                .any(|claim| claim.send_id == record.send_id),
            "the refused claim was retired before its late evidence"
        );
        assert!(
            ledger.has_rpc_handoff(&record.send_id),
            "the RPC binding was cleared"
        );
        assert!(
            crate::service::agent_runtime::user_delivery::is_ready(
                &fixture.service,
                &record.authority
            ),
            "the send authority is not ready"
        );
        assert!(
            fixture.service.user_authority_is_current_session(
                &record.authority,
                &thread,
                &context.binding_nonce
            ),
            "the send authority is not the current session"
        );
        subscriber::publish_events(
            &fixture.service,
            &session,
            context.epoch,
            vec![AgentRuntimeEvent::SendEvidence {
                thread_id: thread.clone(),
                send_id: record.send_id.clone(),
                turn_id: "queued-turn".into(),
                item_id: None,
            }],
            now(),
        )
        .await
        .unwrap();
        let delivered = ledger.status(&record.send_id).unwrap();
        assert_eq!(delivered.state, ChatSendState::Sent);
        assert_eq!(delivered.error, None);
        assert_eq!(
            store.receipts(&tab).unwrap()[0].state,
            ReceiptState::Delivered
        );
        // Nothing can write the refused prompt again: no rebind and no requeued record.
        assert!(!ledger.bind_rpc_send(&record.send_id, SendLiveBinding::default()));
        assert!(store
            .records(&tab)
            .unwrap()
            .iter()
            .all(|item| item.send_id != record.send_id));
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(exercise, server);
    })
    .await
    .expect("busy refusal must settle as uncertain and reconcile from late evidence");
}

#[tokio::test]
async fn refused_claim_survives_the_busy_turn_and_expires_when_the_following_turn_settles() {
    let (fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    let context = session.context.lock().clone();
    let thread = context.thread_id.clone().unwrap();
    for send in ["refused", "unrelated"] {
        session.context.lock().claimed_sends.push(ClaimedSend {
            send_id: send.into(),
            thread_id: thread.clone(),
            input: TurnInput {
                text: format!("opaque-{send}"),
                attachments: vec![],
            },
        });
    }
    session.maps.lock().ensure_epoch(context.epoch);
    session
        .maps
        .lock()
        .remember_refused_claim(context.epoch, "refused", "refused-turn");
    let transition = |turn: &str, state| AgentRuntimeEvent::TurnState {
        thread_id: thread.clone(),
        turn_id: Some(turn.into()),
        parent_thread_id: None,
        state,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
    };
    subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![
            transition("refused-turn", RuntimeTurnState::Working),
            transition("refused-turn", RuntimeTurnState::Idle),
        ],
        now(),
    )
    .await
    .unwrap();
    assert!(session
        .context
        .lock()
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "refused"));
    let has_refused = || {
        session
            .context
            .lock()
            .claimed_sends
            .iter()
            .any(|claim| claim.send_id == "refused")
    };
    for _ in 0..2 {
        subscriber::publish_events(
            &fixture.service,
            &session,
            context.epoch,
            vec![
                transition("busy-turn", RuntimeTurnState::Working),
                transition("busy-turn", RuntimeTurnState::Idle),
            ],
            now(),
        )
        .await
        .unwrap();
        assert!(
            has_refused(),
            "the busy turn may still precede the refused prompt"
        );
    }
    subscriber::publish_events(
        &fixture.service,
        &session,
        context.epoch,
        vec![
            transition("next-turn", RuntimeTurnState::Working),
            transition("next-turn", RuntimeTurnState::Idle),
        ],
        now(),
    )
    .await
    .unwrap();
    let current = session.context.lock();
    assert!(!current
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "refused"));
    assert!(current
        .claimed_sends
        .iter()
        .any(|claim| claim.send_id == "unrelated"));
}
