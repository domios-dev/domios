use super::*;
use super::sync_send::{runtime_fixture_with_stored_history, RUNTIME_FIXTURE_SOURCE};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn fresh_runtime_history_is_empty_before_rollout_and_readable_when_ready() {
    let source = RUNTIME_FIXTURE_SOURCE
        .replace("module.exports = {", "module.exports = { runtimeStoredHistoryAvailable: () => false,")
        .replace("resumeThread:'fixture/resume'", "openThread:'fixture/open',resumeThread:'fixture/resume'")
        .replace("if (op.kind === 'resumeThread')", "if (op.kind === 'openThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:'own-thread',settings:op.settings}}; if (op.kind === 'resumeThread')");
    let (fixture, tab_id, peer, _cleanup) = runtime_fixture_with_stored_history(&source);
    let session = for_tab(&tab_id).unwrap();
    session.thread_loaded.store(false, std::sync::atomic::Ordering::Release);
    session.state.lock().phase = RuntimePhase::Loading;
    session.context.lock().thread_id = None;
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.agent_booting = true;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentLaunching);
        if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
            session_id.clear();
        }
    }
    let owner = fixture.service.clone();
    let target = tab_id.clone();
    let peer = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        for method in ["fixture/initialize", "fixture/open", "fixture/history"] {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line)).await.unwrap().unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            if method == "fixture/history" {
                let tab = owner.mux.read().get_tab(&target).unwrap().clone();
                assert!(!tab.agent_booting);
                assert_eq!(tab.boot_stage, Some(codeterm_types::pty::BootStage::AgentReady));
                let transcript = crate::service::tab_chat::tab_chat_snapshot(&owner, &target, 0).unwrap();
                assert_eq!(transcript, (Vec::new(), 0));
                for config in [serde_json::json!({}), serde_json::json!({"unusedThreadId":"foreign-thread"})] {
                    owner.mux.write().get_tab_mut(&target).unwrap().agent_runtime_config = Some(config);
                    let error = crate::service::tab_chat::tab_chat_snapshot(&owner, &target, 0).unwrap_err();
                    assert_eq!(crate::service::tab_chat::runtime_stored::StoredHistoryFailure::from_error(&error), Some(crate::service::tab_chat::runtime_stored::StoredHistoryFailure::Missing));
                }
                owner.mux.write().get_tab_mut(&target).unwrap().agent_runtime_config = Some(serde_json::json!({"unusedThreadId":"own-thread"}));
            }
            peer.get_mut().write_all(format!("{}\n", serde_json::json!({
                "jsonrpc":"2.0","id":request["id"],"result":{}
            })).as_bytes()).await.unwrap();
        }
    });
    crate::async_runtime::block_on(async {
        ensure_locked(&fixture.service, &tab_id, &session.provider, &serde_json::json!({}), &session, None).await.unwrap();
        peer.await.unwrap();
    });
    let tab = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    assert!(!tab.agent_booting);
    assert_eq!(tab.boot_stage, Some(codeterm_types::pty::BootStage::AgentReady));
    assert!(session.thread_loaded.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap(), (Vec::new(), 0));
}
