use super::*;

const SIGNED_OUT_SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op) {
    return {kind:'complete',result:{kind:op.kind === 'initialize' ? 'initialized' : 'done'},events:[{kind:'auth',state:'signedOut',detail:'opaque-auth-detail'}]};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

#[test]
fn signed_out_restart_is_accepted_pending_without_latching_launch_failure() {
    for fresh in [false, true] {
        let fixture = RuntimeFixture::with_source(SIGNED_OUT_SOURCE);
        let outcome = fixture
            .service
            .agent_restart_observed_with_options(
                &fixture.tab,
                RestartOptions {
                    fresh,
                    force: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(outcome.restart_accepted);
        assert!(!outcome.agent_ready);
        assert!(!outcome.binding_restored);
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        assert_eq!(session.state.lock().snapshot().state, "loginPending");
        assert!(session.state.lock().snapshot().reason.is_none());
        assert_ne!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&fixture.tab)
                .unwrap()
                .boot_stage,
            Some(codeterm_types::pty::BootStage::SpawnFailed)
        );
        assert!(!fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "submitTurn"));
    }
}

#[test]
fn missing_profile_secret_projects_login_pending_and_normal_restart_binds_after_key_added() {
    let fixture = RuntimeFixture::prepared_with_profile(SOURCE, true, true);
    crate::async_runtime::block_on(async {
        *fixture.service.secrets.write().await = Arc::new(crate::secrets::mock::MockStore::new());
    });
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        if let TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
            session_id.clear();
        }
    }
    agent_runtime::transport::launch(
        &fixture.service,
        PROVIDER,
        &fixture.tab,
        json!({"launchProfile":"credential","systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
    let pending = || {
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_state
            .clone()
            .unwrap()
    };
    assert_eq!(pending()["state"], "loginPending");
    assert_eq!(pending()["auth"], "unknown");
    assert_eq!(
        pending()["requiredSecrets"],
        json!(["domios:provider:restart-fixture:key"])
    );
    assert!(fixture.requests().is_empty());
    let still_pending = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert!(still_pending.session_id.is_none());
    assert!(!still_pending.agent_ready);
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
    let outcome = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert!(outcome.agent_ready);
    assert!(outcome
        .session_id
        .as_deref()
        .is_some_and(|id| !id.is_empty()));
    assert_eq!(pending()["state"], "loaded");
    assert!(pending().get("requiredSecrets").is_none());
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "openThread")
            .count(),
        1
    );
}

const KEYED_AUTH_SOURCE: &str = r#"
let signedIn = false;
const declared = state => ({kind:'authentication',authentication:{state,
  methods:[{id:'opaque-key-method',name:'Opaque key',available:true}]}});
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    const auth = {kind:'auth',state:signedIn ? 'signedIn' : 'signedOut'};
    if (op.kind === 'initialize')
      return {kind:'complete',result:{kind:'initialized'},events:[declared(signedIn ? 'authenticated' : 'needs_auth'),auth]};
    if (op.kind === 'control' && op.action === 'login') {
      if (op.value.methodId !== 'opaque-key-method' || reply.value.authKey !== 'opaque-test-key')
        return {kind:'refused',reason:{code:'authentication_required',message:'fixture key missing'},beforeAcceptance:true};
      signedIn = true;
      return {kind:'complete',result:{kind:'done'},events:[declared('authenticated'),{kind:'auth',state:'signedIn'}]};
    }
    if (op.kind === 'control') return {kind:'complete',result:{kind:'done'},events:[auth]};
    if (!signedIn && (op.kind === 'openThread' || op.kind === 'resumeThread'))
      return {kind:'refused',reason:{code:'authentication_required',message:'fixture sign-in required'},beforeAcceptance:true};
    let result = {kind:'done'};
    if (op.kind === 'openThread' || op.kind === 'resumeThread')
      result = {kind:'threadLoaded',threadId:op.threadId || ctx.bindingNonce,settings:op.settings};
    if (op.kind === 'readHistory') result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'submitTurn') result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'turn'};
    return {kind:'complete',result};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

const CLIENT_BRIDGE: &str = r#"{"fsRead":true,"fsWrite":false,"terminal":false}"#;

#[test]
fn restart_retires_the_old_epoch_client_bridge_before_relaunching() {
    let fixture = RuntimeFixture::prepared_with_runtime(
        SOURCE,
        true,
        json!({"clientBridges": serde_json::from_str::<Value>(CLIENT_BRIDGE).unwrap()}),
    );
    agent_runtime::transport::launch(
        &fixture.service,
        PROVIDER,
        &fixture.tab,
        json!({"systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
    let outcome = fixture.service.agent_restart(&fixture.tab, false, 0);
    assert!(
        outcome.as_ref().is_ok_and(|outcome| outcome.agent_ready),
        "{outcome:?}"
    );
}

#[test]
fn declared_key_sign_in_stores_the_key_injects_it_and_authenticates_before_loading() {
    const SECRET: &str = "domios:provider:restart-fixture:auth";
    // The bridge makes the sign-in restart retire the running epoch's bridge, as live ACP does.
    let fixture = RuntimeFixture::prepared_with_runtime(
        KEYED_AUTH_SOURCE,
        true,
        json!({
            "launchProfiles": {"keyed":{"binary":"node","args":[],
                "authEnv":{"OPAQUE_AUTH_KEY":{"secret":SECRET,"methods":["opaque-key-method"]}}}},
            "clientBridges": serde_json::from_str::<Value>(CLIENT_BRIDGE).unwrap()
        }),
    );
    crate::async_runtime::block_on(async {
        *fixture.service.secrets.write().await = Arc::new(crate::secrets::mock::MockStore::new());
    });
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        if let TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
            session_id.clear();
        }
    }
    agent_runtime::transport::launch(
        &fixture.service,
        PROVIDER,
        &fixture.tab,
        json!({"launchProfile":"keyed","systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
    let state = || {
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_state
            .clone()
            .unwrap()
    };
    assert_eq!(state()["state"], "loginPending");
    assert_eq!(state()["apiKeyMethods"], json!(["opaque-key-method"]));
    let injected = || {
        agent_runtime::process::test_ports::spawned_env(&fixture.tab)
            .get("OPAQUE_AUTH_KEY")
            .cloned()
    };
    assert_eq!(
        injected(),
        None,
        "an absent optional key never refuses launch"
    );

    let refused = crate::async_runtime::block_on(agent_runtime::login::operate(
        &fixture.service,
        &fixture.tab,
        serde_json::from_value(json!({"methodId":"opaque-key-method"})).unwrap(),
    ));
    assert!(
        refused.is_err(),
        "authentication without the key is refused"
    );
    assert_eq!(state()["state"], "loginPending");

    let signed_in = crate::async_runtime::block_on(agent_runtime::login::operate(
        &fixture.service,
        &fixture.tab,
        serde_json::from_value(
            json!({"methodId":"opaque-key-method","apiKey":" opaque-test-key\n"}),
        )
        .unwrap(),
    ));
    assert!(signed_in.is_ok(), "{signed_in:?}");
    let stored = crate::async_runtime::block_on(async {
        let store = fixture.service.secrets.read().await.clone();
        store.get(SECRET).await.unwrap().value.into_inner()
    });
    assert_eq!(stored, "opaque-test-key");
    assert_eq!(injected().as_deref(), Some("opaque-test-key"));
    assert_eq!(state()["state"], "loaded");
    let requests = fixture.requests();
    let login = requests
        .iter()
        .rposition(|request| {
            request["params"]["action"] == "login"
                && request["params"]["value"]["methodId"] == "opaque-key-method"
        })
        .expect("the declared method was authenticated");
    assert!(requests[login..]
        .iter()
        .any(|request| request["method"] == "openThread"));
    assert!(!requests
        .iter()
        .any(|request| request.to_string().contains("opaque-test-key")));
}

#[test]
fn bound_runtime_with_missing_tab_identity_keeps_normal_restart_refusal_without_consuming_cooldown()
{
    let fixture = RuntimeFixture::new();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_session_id = None;
    for _ in 0..2 {
        let error = fixture
            .service
            .agent_restart(&fixture.tab, false, 0)
            .unwrap_err();
        assert!(
            matches!(error, AgentRestartError::BadRequest(ref reason) if reason.contains("Runtime thread identity missing")),
            "{error:?}"
        );
    }
    assert!(!fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .contains_key(&fixture.tab));
}
