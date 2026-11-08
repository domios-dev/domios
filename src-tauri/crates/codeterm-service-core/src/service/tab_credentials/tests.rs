// WHAT: launch credential lifetime and authenticated RPC callers.
// WHY: a shared instance bearer permitted cross-tab impersonation.
// ORACLE: only the live launch credential binds its actor; retirement refuses it.
use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use serde_json::json;

fn fixture() -> (ServiceFixture, String, String) {
    let (mut mux, first) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&first).unwrap();
    let second = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    (fixture, first, second)
}

fn token(svc: &super::super::ServiceContext, tab: &str) -> String {
    svc.frontend_pty_env(Some(tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap()
}

#[test]
fn credentials_are_distinct_per_tab_and_never_the_host_secret() {
    let (fixture, first, second) = fixture();
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    let first_token = token(svc, &first);
    let second_token = token(svc, &second);
    assert_ne!(first_token, second_token);
    assert_ne!(first_token, "owner-secret");
    assert!(svc
        .mint_tab_credential(&first, "superseded-launch")
        .is_none());
    assert_eq!(token(svc, &first), first_token);
    assert_eq!(
        svc.identify_tab_credential(&first_token).unwrap().tab_id,
        first
    );
    assert_eq!(
        svc.identify_tab_credential(&second_token).unwrap().tab_id,
        second
    );
    assert!(!svc
        .frontend_pty_env(None)
        .unwrap()
        .contains_key("CODETERM_API_TOKEN"));
    assert!(svc.mint_tab_credential("missing-tab", "nonce").is_none());
    assert!(svc.identify_tab_credential("").is_none());
}

#[test]
fn plugin_children_use_the_pty_launch_credential_with_agent_rpc_authority() {
    use crate::multiplexer::types::TabBacking;
    use codeterm_plugin::host_traits::PluginHostCtx;
    use codeterm_plugin::js_runtime::{JsChatBackend, PluginVm};
    use std::sync::Arc;

    let (fixture, tab, other) = fixture();
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    let previous = token(svc, &tab);
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_type = Some("credential-probe".into());
        target.backing = TabBacking::ChatBackend {
            plugin_id: "credential-probe".into(),
            session_id: String::new(),
        };
    }
    let host: Arc<dyn PluginHostCtx> = Arc::new(svc.clone());
    let vm = Arc::new(PluginVm::from_source("credential-probe", r#"
        module.exports.default = {
            openSession() {
                const opts = host.platform() === "windows"
                    ? { bin: "powershell", args: ["-NoProfile", "-Command", "[Console]::Out.Write($env:CODETERM_API_TOKEN)"] }
                    : { bin: "sh", args: ["-c", "printf %s \"$CODETERM_API_TOKEN\""] };
                opts.env = { CODETERM_API_TOKEN: "owner-secret" };
                return host.exec.async(opts, function (out) {
                    if (out.code !== 0) throw new Error(JSON.stringify(out));
                    return { sessionId: out.stdout };
                });
            }
        };
    "#.into()).unwrap());
    assert!(vm.set_service(Arc::downgrade(&host)));
    let backend = Arc::new(JsChatBackend::new(vm, "credential-probe"));
    svc.chatbackend_registry
        .register("credential-probe", backend);
    let credential = svc
        .chatbackend_open_session("credential-probe", &tab, json!({}))
        .unwrap();
    let launch_nonce = svc
        .mux
        .read()
        .get_tab(&tab)
        .unwrap()
        .session_binding_nonce
        .clone()
        .unwrap();
    svc.mux.write().get_tab_mut(&tab).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "credential-probe".into(),
        session_id: credential.clone(),
    };
    assert_eq!(
        svc.bind_in_process_chatbackend_session(&tab, "credential-probe", &credential)
            .unwrap(),
        launch_nonce
    );
    assert!(svc.authenticate_local_credential(&previous).is_err());
    assert_eq!(credential, token(svc, &tab));
    let env = svc.tab_process_env(&tab, &launch_nonce).unwrap();
    assert_eq!(env["CODETERM_API_TOKEN"], credential);
    assert_ne!(credential, "owner-secret");
    let caller = svc
        .authenticate_local_credential(&credential)
        .unwrap()
        .unwrap();
    assert_eq!(caller.tab_id, tab);
    assert_eq!(caller.binding_nonce, env["CODETERM_SESSION_BINDING_NONCE"]);
    assert!(!svc
        .tab_process_env("missing-tab", "missing-nonce")
        .unwrap()
        .contains_key("CODETERM_API_TOKEN"));
    for method in [
        "agent.ack",
        "agent.report",
        "tab.list",
        "mem.search",
        "plan.get",
    ] {
        let mut params = Some(json!({}));
        svc.bind_local_rpc(&credential, method, &mut params)
            .unwrap();
        assert_eq!(params.unwrap()["caller_tab_id"], tab, "{method}");
    }
    let mut send = Some(json!({"tab_id":other,"text":"opaque"}));
    svc.bind_local_rpc(&credential, "tab.send_text", &mut send)
        .unwrap();
    assert_eq!(send.unwrap()["source_tab"], tab);
    assert!(svc
        .bind_local_rpc(&credential, "rest_api.restart", &mut None)
        .is_err());
    let reopened = svc
        .chatbackend_open_session("credential-probe", &tab, json!({}))
        .unwrap();
    assert!(svc
        .bind_in_process_chatbackend_session(&tab, "credential-probe", &credential)
        .is_err());
    svc.mux.write().get_tab_mut(&tab).unwrap().backing = TabBacking::ChatBackend {
        plugin_id: "credential-probe".into(),
        session_id: reopened.clone(),
    };
    let reopened_nonce = svc
        .bind_in_process_chatbackend_session(&tab, "credential-probe", &reopened)
        .unwrap();
    assert_ne!(reopened_nonce, launch_nonce);
    assert!(svc.tab_process_env(&tab, &launch_nonce).unwrap().is_empty());
    assert!(svc.authenticate_local_credential(&credential).is_err());
    assert_eq!(
        svc.tab_process_env(&tab, &reopened_nonce).unwrap()["CODETERM_API_TOKEN"],
        reopened
    );
    let mut ack = None;
    svc.bind_local_rpc(&reopened, "agent.ack", &mut ack)
        .unwrap();
    assert_eq!(ack.unwrap()["tab_id"], tab);
    svc.rotate_session_binding_nonce(&tab);
    assert!(svc.authenticate_local_credential(&reopened).is_err());
}

#[test]
fn launch_rotation_and_close_revoke_previous_credentials() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    let previous = token(svc, &tab);
    svc.rotate_session_binding_nonce(&tab);
    assert!(svc.identify_tab_credential(&previous).is_none());
    let current = token(svc, &tab);
    assert_ne!(current, previous);
    assert!(svc.identify_tab_credential(&current).is_some());
    svc.close_tab(&tab).unwrap();
    assert!(svc.identify_tab_credential(&current).is_none());
    assert!(svc.mint_tab_credential(&tab, "nonce").is_none());
}

#[test]
fn first_session_binding_preserves_launch_but_session_replacement_revokes_it() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    let launch = token(svc, &tab);
    svc.set_agent_session_id(&tab, Some("first-session".into()));
    assert!(svc.identify_tab_credential(&launch).is_some());
    svc.set_agent_session_id(&tab, Some("second-session".into()));
    assert!(svc.identify_tab_credential(&launch).is_none());
}

#[test]
fn materialized_rebind_issues_current_launch_credentials_without_a_retry() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-runtime".into());
    tab.agent_session_id = Some("previous-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let previous = token(svc, &tab_id);
    let previous_file =
        std::path::PathBuf::from(svc.runtime_credential_file(&tab_id, "own-nonce").unwrap());
    {
        let mut mux = svc.mux.write();
        let evidence = crate::agent::provider_identity::ProviderIdentityEvidence::capture(
            mux.get_tab(&tab_id).unwrap(),
        );
        let claim = crate::service::hook_session_binding::commit_materialized_rebind_session(
            &mut mux,
            &tab_id,
            "fixture-runtime",
            "rebound-thread",
            None,
            &evidence,
        )
        .unwrap();
        assert!(claim.changed);
    }
    let current = svc.mint_tab_credential(&tab_id, "own-nonce").unwrap();
    assert_ne!(current, previous);
    assert!(!previous_file.exists());
    assert!(svc.identify_tab_credential(&previous).is_none());
    let caller = svc.identify_tab_credential(&current).unwrap();
    assert_eq!(caller.tab_id, tab_id);
    assert_eq!(caller.binding_nonce, "own-nonce");
    assert_eq!(token(svc, &tab_id), current);
    let current_file =
        std::path::PathBuf::from(svc.runtime_credential_file(&tab_id, "own-nonce").unwrap());
    assert_eq!(
        codeterm_util::tab_launch::resolve(
            None,
            Some(&current_file),
            Some(&tab_id),
            Some("own-nonce")
        )
        .unwrap(),
        current
    );
    assert!(svc.mint_tab_credential(&tab_id, "foreign-nonce").is_none());
    assert!(svc.identify_tab_credential(&current).is_some());
    svc.rotate_session_binding_nonce(&tab_id);
    assert!(!current_file.exists());
    assert!(svc.identify_tab_credential(&current).is_none());
    assert!(svc.mint_tab_credential(&tab_id, "own-nonce").is_none());
}

#[test]
fn daemon_restart_does_not_restore_a_launch_credential() {
    let registry = TabCredentials::default();
    let identity = TabCaller {
        tab_id: "tab".into(),
        binding_nonce: "persisted-nonce".into(),
    };
    let previous = registry.mint(identity.clone(), None, false);
    assert_eq!(registry.identify(&previous), Some(identity.clone()));
    let restarted = TabCredentials::default();
    assert!(restarted.identify(&previous).is_none());
    let current = restarted.mint(identity, None, false);
    assert_ne!(previous, current);
    assert!(restarted.identify(&previous).is_none());
}

#[test]
fn rpc_derives_actor_when_claims_are_omitted_and_refuses_foreign_claims() {
    let (fixture, tab, other) = fixture();
    let svc = &fixture.service;
    let credential = token(svc, &tab);
    let mut params = Some(json!({"tab_id": other, "text":"opaque-payload"}));
    svc.bind_local_rpc(&credential, "tab.send_text", &mut params)
        .unwrap();
    let params = params.unwrap();
    assert_eq!(params["caller_tab_id"], tab);
    assert_eq!(params["source_tab"], tab);
    assert_eq!(params["tab_id"], other);
    for field in [
        "caller_tab_id",
        "source_tab",
        "creator_tab_id",
        "spawned_by_tab_id",
        "asked_by_tab_id",
        "answered_by_tab_id",
    ] {
        let mut forged = Some(json!({field: other}));
        assert!(
            svc.bind_local_rpc(&credential, "tab.send_text", &mut forged)
                .is_err(),
            "{field}"
        );
    }
    let mut forged_report = Some(json!({"tab_id": other}));
    assert!(svc
        .bind_local_rpc(&credential, "agent.report", &mut forged_report)
        .is_err());
    let mut stale_nonce = Some(json!({"session_binding_nonce":"foreign-nonce"}));
    assert!(svc
        .bind_local_rpc(&credential, "agent.ack", &mut stale_nonce)
        .is_err());
}

#[test]
fn owner_credential_cannot_claim_a_tab_and_local_api_keeps_the_presented_credential() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    let credential = token(svc, &tab);
    let mut forged = Some(json!({"caller_tab_id": tab}));
    assert!(svc
        .bind_local_rpc("owner-secret", "agent.report", &mut forged)
        .is_err());
    let mut params = Some(
        json!({"method":"GET","path":"/api/config","authenticated_api_credential":"owner-secret"}),
    );
    svc.bind_local_rpc(&credential, "api.request", &mut params)
        .unwrap();
    assert_eq!(params.unwrap()["authenticated_api_credential"], credential);
    let mut owner = None;
    svc.bind_local_rpc("owner-secret", "system.identify", &mut owner)
        .unwrap();
    assert!(svc
        .bind_local_rpc("unknown", "system.identify", &mut owner)
        .is_err());
}

#[test]
fn credential_pins_the_first_attested_session_and_refuses_same_nonce_replacement() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    let credential = token(svc, &tab);
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&tab).unwrap();
        tab.agent_session_id = Some("attested-session".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    assert!(svc.identify_tab_credential(&credential).is_some());
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_session_id =
        Some("replacement-session".into());
    assert!(svc.identify_tab_credential(&credential).is_none());
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_session_id = Some("attested-session".into());
    assert!(svc.identify_tab_credential(&credential).is_none());
}

#[test]
fn nested_batch_actor_claims_are_bound_and_foreign_claims_refuse() {
    let (fixture, tab, other) = fixture();
    let svc = &fixture.service;
    let credential = token(svc, &tab);
    let mut params = Some(json!({"ops":[{"op":"add_milestone","task":"owned","kind":"note"}]}));
    svc.bind_local_rpc(&credential, "plan.batch", &mut params)
        .unwrap();
    assert_eq!(params.unwrap()["ops"][0]["from"]["tab_id"], tab);
    let mut forged = Some(
        json!({"ops":[{"op":"add_milestone","task":"owned","kind":"note","from":{"tab_id":other}}]}),
    );
    assert!(svc
        .bind_local_rpc(&credential, "plan.batch", &mut forged)
        .is_err());
}

#[test]
fn tab_system_rpc_accepts_null_params_without_synthetic_caller_fields() {
    let (fixture, tab, _) = fixture();
    let credential = token(&fixture.service, &tab);
    let mut params = Some(serde_json::Value::Null);
    fixture
        .service
        .bind_local_rpc(&credential, "system.identify", &mut params)
        .unwrap();
    assert_eq!(params, Some(json!({})));
}

#[test]
fn new_launch_can_bind_its_first_session_over_a_retained_previous_session() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    svc.set_agent_session_id(&tab, Some("previous-launch-session".into()));
    svc.rotate_session_binding_nonce(&tab);
    let credential = token(svc, &tab);
    svc.set_agent_session_id(&tab, Some("new-launch-session".into()));
    assert!(svc.identify_tab_credential(&credential).is_some());
    svc.set_agent_session_id(&tab, Some("unrelated-session".into()));
    assert!(svc.identify_tab_credential(&credential).is_none());
}

#[test]
fn omitted_plan_actor_references_derive_the_authenticated_tab() {
    let (fixture, tab, _) = fixture();
    let credential = token(&fixture.service, &tab);
    for (method, field) in [
        ("plan.create", "owner"),
        ("plan.handoff.create", "from"),
        ("plan.milestone.add", "from"),
    ] {
        let mut params = Some(json!({}));
        fixture
            .service
            .bind_local_rpc(&credential, method, &mut params)
            .unwrap();
        assert_eq!(params.unwrap()[field]["tab_id"], tab, "{method}");
    }
}

#[test]
fn owner_only_rpc_rejects_tab_authority_before_binding_params() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    let credential = token(svc, &tab);
    for method in [
        "model_provider.discover",
        "model_provider.list",
        "model_provider.future",
        "rest_api.restart",
    ] {
        for mut params in [
            None,
            Some(json!(null)),
            Some(json!({"authority":"Local","authenticated_api_credential":"owner-secret"})),
        ] {
            let original = params.clone();
            assert_eq!(
                svc.bind_local_rpc(&credential, method, &mut params),
                Err("RPC requires the local owner credential".into())
            );
            assert_eq!(params, original);
            svc.bind_local_rpc("owner-secret", method, &mut params)
                .unwrap();
            assert_eq!(params, original);
        }
    }
    for method in ["system.identify", "system.capabilities", "rest_api.status"] {
        svc.bind_local_rpc(&credential, method, &mut None).unwrap();
    }
}
