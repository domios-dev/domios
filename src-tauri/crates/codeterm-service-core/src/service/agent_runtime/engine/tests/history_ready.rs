use super::sync_send::{
    runtime_fixture_with_stored_history, runtime_fixture_without_remote_history,
    RUNTIME_FIXTURE_SOURCE,
};
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
