use super::super::*;
use super::sync_send::runtime_fixture_with;
use crate::service::agent_runtime::{login, transport};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const method = {initialize:'fixture/initialize',openThread:'fixture/open',resumeThread:'fixture/resume',readHistory:'fixture/history',control:'fixture/control'}[op.kind];
    return {action:{kind:'request',call:{method,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') return {kind:'complete',result:{kind:'initialized'},events:[{kind:'auth',state:'signedOut'}]};
    if (op.kind === 'control') {
      if (op.action === 'auth') return {kind:'complete',result:{kind:'done'},events:[{kind:'auth',state:reply.value.auth}]};
      const value = op.value;
      const login = value.action === 'cancel' ? {status:'canceled',loginId:value.loginId}
        : value.method === 'apiKey' ? {status:'completed',method:'apiKey'}
        : {status:'pending',method:value.method,loginId:'opaque-login',url:'https://example.invalid/opaque',...(value.method === 'device' ? {userCode:'opaque-code'} : {})};
      return {kind:'complete',result:{kind:'done'},events:[{kind:'authLogin',login}]};
    }
    if (op.kind === 'openThread' || op.kind === 'resumeThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:op.threadId || 'opaque-new-thread',settings:op.settings}};
    return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:[],openItems:[],order:op.order}};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;

#[test]
fn signed_out_launch_attaches_cancellation_stays_pending_and_login_resumes_same_epoch() {
    for method in [
        RuntimeLoginMethod::Browser,
        RuntimeLoginMethod::Device,
        RuntimeLoginMethod::ApiKey,
    ] {
        for fresh in [false, true] {
            let (fixture, id, peer, _cleanup) = runtime_fixture_with(SOURCE);
            let session = for_tab(&id).unwrap();
            session.state.lock().phase = RuntimePhase::Loading;
            session.thread_loaded.store(false, Ordering::Release);
            if fresh {
                session.state.lock().thread_id = None;
                session.context.lock().thread_id = None;
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(&id)
                    .unwrap()
                    .agent_session_id = None;
            }
            fixture.install_seeded_storage_desk();
            let signed_in = Arc::new(AtomicBool::new(false));
            let account = signed_in.clone();
            let server = crate::async_runtime::spawn(async move {
                let mut peer = BufReader::new(peer);
                loop {
                    let mut line = String::new();
                    if peer.read_line(&mut line).await.unwrap() == 0 {
                        break;
                    }
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let response = json!({"id":request["id"],"result":{"auth":if account.load(Ordering::Acquire) { "signedIn" } else { "signedOut" }}});
                    peer.get_mut()
                        .write_all(format!("{response}\n").as_bytes())
                        .await
                        .unwrap();
                }
            });
            transport::launch(&fixture.service, &session.provider, &id, json!({})).unwrap();
            let epoch = session.context.lock().epoch;
            assert_eq!(session.state.lock().snapshot().state, "loginPending");
            assert!(session.state.lock().snapshot().reason.is_none());
            assert_ne!(
                fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
                Some(codeterm_types::pty::BootStage::SpawnFailed)
            );
            let (attached, items) = transport::snapshot(&fixture.service, &id).unwrap();
            assert!(Arc::ptr_eq(&session, &attached));
            assert!(items.is_empty());
            crate::async_runtime::block_on(async {
                let start = || login::LoginRequest {
                    provider_id: None,
                    account: None,
                    code: None,
                    action: None,
                    method: Some(method.clone()),
                    api_key: (method == RuntimeLoginMethod::ApiKey).then(|| "opaque-secret".into()),
                };
                if method != RuntimeLoginMethod::ApiKey {
                    let value = login::operate(&fixture.service, &id, start())
                        .await
                        .unwrap();
                    assert_eq!(value["login"]["status"], "pending");
                    assert_eq!(login::show(&fixture.service, &id).unwrap(), value);
                    let own_nonce = fixture
                        .service
                        .mux
                        .write()
                        .get_tab_mut(&id)
                        .unwrap()
                        .session_binding_nonce
                        .replace("foreign-nonce".into());
                    assert!(login::show(&fixture.service, &id).is_err());
                    fixture
                        .service
                        .mux
                        .write()
                        .get_tab_mut(&id)
                        .unwrap()
                        .session_binding_nonce = own_nonce;
                    assert!(value["login"]["expiresAtMs"].as_u64().unwrap() > login::now_ms());
                    session.login.lock().phone_notification = Some("opaque-notification".into());
                    let notification = login::operate(
                        &fixture.service,
                        &id,
                        login::LoginRequest {
                            provider_id: None,
                            account: None,
                            code: None,
                            action: Some("notify".into()),
                            method: None,
                            api_key: None,
                        },
                    )
                    .await
                    .unwrap();
                    assert_eq!(
                        notification,
                        json!({"notificationId":"opaque-notification"})
                    );
                    assert_eq!(value["login"]["url"], "https://example.invalid/opaque");
                    if method == RuntimeLoginMethod::Device {
                        assert_eq!(value["login"]["userCode"], "opaque-code");
                    }
                    login::operate(
                        &fixture.service,
                        &id,
                        login::LoginRequest {
                            provider_id: None,
                            account: None,
                            code: None,
                            action: Some("cancel".into()),
                            method: None,
                            api_key: None,
                        },
                    )
                    .await
                    .unwrap();
                    let canceled = login::show(&fixture.service, &id).unwrap();
                    assert!(canceled["login"].get("url").is_none());
                    assert!(canceled["login"].get("userCode").is_none());
                    assert_eq!(session.state.lock().snapshot().state, "loginPending");
                    assert_eq!(
                        session.login.lock().login.as_ref().unwrap().status,
                        RuntimeLoginStatus::Canceled
                    );
                }
                signed_in.store(true, Ordering::Release);
                let mut changes = session.control_changes.subscribe();
                login::operate(&fixture.service, &id, start())
                    .await
                    .unwrap();
                if method != RuntimeLoginMethod::ApiKey {
                    let mut completion = session.login.lock().login.clone().unwrap();
                    completion.status = RuntimeLoginStatus::Completed;
                    login::apply(&fixture.service, &session, epoch, completion);
                }
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if matches!(session.state.lock().phase, RuntimePhase::Ready) {
                            break;
                        }
                        changes.changed().await.unwrap();
                    }
                })
                .await
                .unwrap();
            });
            assert_eq!(session.context.lock().epoch, epoch);
            assert_eq!(
                session.context.lock().thread_id.as_deref(),
                Some(if fresh {
                    "opaque-new-thread"
                } else {
                    "own-thread"
                })
            );
            assert_eq!(
                fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
                Some(codeterm_types::pty::BootStage::AgentReady)
            );
            *session.auth.lock() = RuntimeAuthState::SignedOut;
            session
                .state
                .lock()
                .degrade(epoch, &RuntimeFailure::SignInRequired);
            crate::async_runtime::block_on(ensure(&fixture.service, &id)).unwrap();
            assert_eq!(session.context.lock().epoch, epoch);
            assert_eq!(session.state.lock().snapshot().state, "loaded");
            server.abort();
        }
    }
}
