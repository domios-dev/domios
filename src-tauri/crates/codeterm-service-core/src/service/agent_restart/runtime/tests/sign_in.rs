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
