use super::super::*;
use super::sync_send::runtime_fixture_with;
use crate::service::agent_runtime::{login, transport};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[path = "sign_in/remote_environments.rs"]
mod remote_environments;

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
            exercise_login(method.clone(), fresh, None);
        }
    }
    for outcome in ["empty", "many", "error"] {
        exercise_login(RuntimeLoginMethod::Browser, false, Some(outcome));
    }
}

fn exercise_login(method: RuntimeLoginMethod, fresh: bool, outcome: Option<&str>) {
    let (fixture, id, peer, _cleanup) = runtime_fixture_with(SOURCE);
    let remote = outcome.map(|outcome| remote_environments::register(&fixture, outcome));
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
            owner_confirmed: false,
            binding: None,
            login_id: None,
            method_id: None,
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
                    owner_confirmed: false,
                    binding: None,
                    login_id: None,
                    method_id: None,
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
                    owner_confirmed: false,
                    binding: None,
                    login_id: None,
                    method_id: None,
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
            if let Some(remote) = &remote {
                remote_environments::assert_calls(remote, 0);
                login::operate(&fixture.service, &id, start())
                    .await
                    .unwrap();
                let mut failure = session.login.lock().login.clone().unwrap();
                failure.status = RuntimeLoginStatus::Failed;
                let mut changes = session.control_changes.subscribe();
                login::apply(&fixture.service, &session, epoch, failure);
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if session.login.lock().login.as_ref().unwrap().status
                            == RuntimeLoginStatus::Failed
                        {
                            break;
                        }
                        changes.changed().await.unwrap();
                    }
                })
                .await
                .unwrap();
                remote_environments::assert_calls(remote, 0);
            }
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
    if let Some(remote) = &remote {
        remote_environments::assert_calls(remote, 1);
        remote_environments::assert_snapshot(&fixture, outcome.unwrap());
    }
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

#[test]
fn stale_signed_out_runtime_respawns_once_when_credentials_appear_on_disk() {
    use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
    use crate::service::{
        delivery_error::DeliveryError as StructuredDeliveryError,
        message_gateway::{classify, DeliveryError as GatewayDeliveryError},
        messenger::{Delivery, MessageEnvelope, Messenger},
        test_fixture::ServiceFixture,
    };
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use std::sync::atomic::AtomicUsize;

    const PROVIDER: &str = "stale-auth-fixture";
    const STALE_AUTH_SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const method = {
      initialize: 'fixture/initialize',
      openThread: 'fixture/open',
      resumeThread: 'fixture/resume',
      readHistory: 'fixture/history',
      control: 'fixture/control',
      submitTurn: 'fixture/submit',
    }[op.kind];
    return { action: { kind: 'request', call: { method, params: op } }, state: null };
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') {
      return {
        kind: 'complete',
        result: { kind: 'initialized' },
        events: [{ kind: 'auth', state: reply.value.auth }],
      };
    }
    if (op.kind === 'control') {
      if (op.action === 'auth') {
        return {
          kind: 'complete',
          result: { kind: 'done' },
          events: [{ kind: 'auth', state: reply.value.auth }],
        };
      }
      const value = op.value;
      const login = value.action === 'cancel'
        ? { status: 'canceled', loginId: value.loginId }
        : {
            status: 'pending',
            method: value.method,
            loginId: 'opaque-login',
            url: 'https://example.invalid/opaque',
            ...(value.method === 'device' ? { userCode: 'opaque-code' } : {}),
          };
      return { kind: 'complete', result: { kind: 'done' }, events: [{ kind: 'authLogin', login }] };
    }
    if (op.kind === 'openThread' || op.kind === 'resumeThread') {
      return {
        kind: 'complete',
        result: { kind: 'threadLoaded', threadId: op.threadId || 'opaque-thread', settings: op.settings },
      };
    }
    if (op.kind === 'submitTurn') {
      return {
        kind: 'complete',
        result: { kind: 'turnAccepted', threadId: op.threadId, sendId: op.sendId, turnId: reply.value.turnId },
      };
    }
    return { kind: 'complete', result: { kind: 'historyPage', threadId: op.threadId, items: [], openItems: [], order: op.order } };
  },
  runtimeReduce() { return { state: null, events: [] }; }
};
"#;

    struct Stop(String);
    impl Drop for Stop {
        fn drop(&mut self) {
            test_support::stop_test(&self.0);
        }
    }

    let home = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some(
            crate::test_support::fixture_manifest_dir()
                .to_string_lossy()
                .into_owned()
                .into(),
        );
        tab.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: String::new(),
        };
    }
    let fixture = ServiceFixture::new(mux);
    let _storage = fixture.install_seeded_storage_desk();
    let binary = std::env::current_exe().unwrap();
    let manifest = PluginManifest::from_json_str(
        &json!({
            "id": PROVIDER,
            "version": "1.0.0",
            "displayName": "Stale Auth Fixture",
            "accounts": {
                "homeEnvironment": "FIXTURE_STALE_AUTH_HOME",
                "defaultHome": home.path().to_string_lossy(),
                "secretEnvironment": [],
                "isolatedPaths": ["auth.json"],
                "credentialFiles": ["auth.json"],
                "sharedPaths": [],
                "continuation": "unverified"
            },
            "capabilities": {
                "agentProvider": true,
                "chatBackend": { "spawnable": true, "sessionConfig": true, "agentTasks": true },
                "agentRuntime": {
                    "runtime": "stdio-jsonrpc",
                    "command": { "binary": binary, "args": [] },
                    "login": { "methods": ["browser", "device", "apiKey"] },
                    "requestMethods": {
                        "control": ["fixture/control"],
                        "initialize": ["fixture/initialize"],
                        "openThread": ["fixture/open"],
                        "resumeThread": ["fixture/resume"],
                        "readHistory": ["fixture/history"],
                        "submitTurn": ["fixture/submit"]
                    },
                    "notificationMethods": []
                }
            },
            "permissions": { "subprocess": { "allow": [binary] } }
        })
        .to_string(),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, STALE_AUTH_SOURCE.into()).unwrap()),
        )));

    let spawns = Arc::new(AtomicUsize::new(0));
    let spawn_count = spawns.clone();
    let cred_file = home.path().join("auth.json");
    let _ports = crate::service::agent_runtime::process::test_ports::register(&tab_id, move || {
        spawn_count.fetch_add(1, Ordering::SeqCst);
        let cached_signed_in = cred_file.exists();
        let (process, peer) = crate::service::agent_runtime::process::test_ports::connected();
        crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            loop {
                let mut line = String::new();
                match peer.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let result = if request["method"] == "fixture/submit" {
                    json!({ "turnId": "accepted-turn" })
                } else {
                    json!({ "auth": if cached_signed_in { "signedIn" } else { "signedOut" } })
                };
                let response = json!({ "id": request["id"], "result": result });
                if peer
                    .get_mut()
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        process
    });
    let _stop = Stop(tab_id.clone());

    transport::launch(&fixture.service, PROVIDER, &tab_id, json!({})).unwrap();
    let session = for_tab(&tab_id).unwrap();
    let initial_epoch = session.context.lock().epoch;
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    assert_eq!(session.state.lock().snapshot().state, "loginPending");

    assert!(matches!(
        crate::async_runtime::block_on(ensure(&fixture.service, &tab_id)),
        Err(RuntimeFailure::SignInRequired)
    ));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);

    let frame = Messenger::from(MessageEnvelope::from_codeterm()).framed_delivery("opaque turn");
    let refused =
        transport::send_envelope(&fixture.service, &tab_id, &frame, "tok-signed-out", None)
            .unwrap_err();
    assert!(
        refused.starts_with("tab_busy_refused:"),
        "unexpected refusal: {refused}"
    );
    let gateway_err = classify(&tab_id, refused);
    assert!(matches!(
        gateway_err,
        GatewayDeliveryError::BusyRefused { .. }
    ));
    let rendered = gateway_err.to_string();
    assert!(!rendered.starts_with("transport error:"));
    assert!(!StructuredDeliveryError::from_opaque(&tab_id, rendered).bytes_written());
    assert_eq!(spawns.load(Ordering::SeqCst), 1);

    crate::async_runtime::block_on(login::operate(
        &fixture.service,
        &tab_id,
        login::LoginRequest {
            owner_confirmed: false,
            binding: None,
            login_id: None,
            method_id: None,
            provider_id: None,
            account: None,
            code: None,
            action: None,
            method: Some(RuntimeLoginMethod::Device),
            api_key: None,
        },
    ))
    .unwrap();
    std::fs::write(home.path().join("auth.json"), b"opaque-credential").unwrap();

    assert!(matches!(
        crate::async_runtime::block_on(ensure(&fixture.service, &tab_id)),
        Err(RuntimeFailure::SignInRequired)
    ));
    assert_eq!(spawns.load(Ordering::SeqCst), 1);

    crate::async_runtime::block_on(login::operate(
        &fixture.service,
        &tab_id,
        login::LoginRequest {
            owner_confirmed: false,
            binding: None,
            login_id: None,
            method_id: None,
            provider_id: None,
            account: None,
            code: None,
            action: Some("cancel".into()),
            method: None,
            api_key: None,
        },
    ))
    .unwrap();

    let delivered =
        transport::send_envelope(&fixture.service, &tab_id, &frame, "tok-signed-in", None).unwrap();
    assert!(matches!(delivered, Delivery::ChatBackend { .. }));
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    assert!(session.context.lock().epoch > initial_epoch);
    assert_eq!(session.state.lock().snapshot().state, "loaded");
}

#[test]
fn remote_task_bound_runtime_launch_refuses_signed_out_provider_instead_of_login_pending() {
    let (fixture, id, peer, _cleanup) = runtime_fixture_with(SOURCE);
    let session = for_tab(&id).unwrap();
    session.state.lock().phase = RuntimePhase::Loading;
    session.thread_loaded.store(false, Ordering::Release);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.report_target_mesh_peer = Some("origin-peer".into());
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-remote".into()),
            task_id: Some("task-remote".into()),
            attempt_id: Some("attempt-remote".into()),
            spawn_handle: None,
            bound_at: 1,
        });
    }
    let server = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        loop {
            let mut line = String::new();
            if peer.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            let response = json!({"id":request["id"],"result":{"auth":"signedOut"}});
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    let error = transport::launch_with_context(
        &fixture.service,
        &session.provider,
        &id,
        json!({}),
        transport::LaunchContext {
            require_signed_in: true,
            ..Default::default()
        },
    )
    .expect_err("remote task-bound runtime launch must refuse signed-out provider");
    assert!(
        transport::is_sign_in_required_launch_error(&error),
        "unexpected error: {error}"
    );
    transport::launch(&fixture.service, &session.provider, &id, json!({}))
        .expect("ordinary relaunch can attach the signed-out worker");
    assert_eq!(session.state.lock().snapshot().state, "loginPending");
    server.abort();
}
