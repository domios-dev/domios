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
    svc.frontend_pty_env(Some(tab)).remove("CODETERM_API_TOKEN").unwrap()
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
    assert!(svc.mint_tab_credential(&first, "superseded-launch").is_none());
    assert_eq!(token(svc, &first), first_token);
    assert_eq!(svc.identify_tab_credential(&first_token).unwrap().tab_id, first);
    assert_eq!(svc.identify_tab_credential(&second_token).unwrap().tab_id, second);
    assert!(!svc.frontend_pty_env(None).contains_key("CODETERM_API_TOKEN"));
    assert!(svc.mint_tab_credential("missing-tab", "nonce").is_none());
    assert!(svc.identify_tab_credential("").is_none());
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
fn daemon_restart_does_not_restore_a_launch_credential() {
    let registry = TabCredentials::default();
    let identity = TabCaller {tab_id: "tab".into(), binding_nonce: "persisted-nonce".into()};
    let previous = registry.mint(identity.clone());
    assert_eq!(registry.identify(&previous), Some(identity.clone()));
    let restarted = TabCredentials::default();
    assert!(restarted.identify(&previous).is_none());
    let current = restarted.mint(identity);
    assert_ne!(previous, current);
    assert!(restarted.identify(&previous).is_none());
}

#[test]
fn rpc_derives_actor_when_claims_are_omitted_and_refuses_foreign_claims() {
    let (fixture, tab, other) = fixture();
    let svc = &fixture.service;
    let credential = token(svc, &tab);
    let mut params = Some(json!({"tab_id": other, "text":"opaque-payload"}));
    svc.bind_local_rpc(&credential, "tab.send_text", &mut params).unwrap();
    let params = params.unwrap();
    assert_eq!(params["caller_tab_id"], tab);
    assert_eq!(params["source_tab"], tab);
    assert_eq!(params["tab_id"], other);
    for field in ["caller_tab_id", "source_tab", "creator_tab_id", "spawned_by_tab_id", "asked_by_tab_id", "answered_by_tab_id"] {
        let mut forged = Some(json!({field: other}));
        assert!(svc.bind_local_rpc(&credential, "tab.send_text", &mut forged).is_err(), "{field}");
    }
    let mut forged_report = Some(json!({"tab_id": other}));
    assert!(svc.bind_local_rpc(&credential, "agent.report", &mut forged_report).is_err());
    let mut stale_nonce = Some(json!({"session_binding_nonce":"foreign-nonce"}));
    assert!(svc.bind_local_rpc(&credential, "agent.ack", &mut stale_nonce).is_err());
}

#[test]
fn owner_credential_cannot_claim_a_tab_and_local_api_keeps_the_presented_credential() {
    let (fixture, tab, _) = fixture();
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    let credential = token(svc, &tab);
    let mut forged = Some(json!({"caller_tab_id": tab}));
    assert!(svc.bind_local_rpc("owner-secret", "agent.report", &mut forged).is_err());
    let mut params = Some(json!({"method":"GET","path":"/api/config","authenticated_api_credential":"owner-secret"}));
    svc.bind_local_rpc(&credential, "api.request", &mut params).unwrap();
    assert_eq!(params.unwrap()["authenticated_api_credential"], credential);
    let mut owner = None;
    svc.bind_local_rpc("owner-secret", "system.identify", &mut owner).unwrap();
    assert!(svc.bind_local_rpc("unknown", "system.identify", &mut owner).is_err());
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
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_session_id = Some("replacement-session".into());
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
    svc.bind_local_rpc(&credential, "plan.batch", &mut params).unwrap();
    assert_eq!(params.unwrap()["ops"][0]["from"]["tab_id"], tab);
    let mut forged = Some(json!({"ops":[{"op":"add_milestone","task":"owned","kind":"note","from":{"tab_id":other}}]}));
    assert!(svc.bind_local_rpc(&credential, "plan.batch", &mut forged).is_err());
}

#[test]
fn tab_system_rpc_accepts_null_params_without_synthetic_caller_fields() {
    let (fixture, tab, _) = fixture();
    let credential = token(&fixture.service, &tab);
    let mut params = Some(serde_json::Value::Null);
    fixture.service.bind_local_rpc(&credential, "system.identify", &mut params).unwrap();
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
    for (method, field) in [("plan.create", "owner"), ("plan.handoff.create", "from"), ("plan.milestone.add", "from")] {
        let mut params = Some(json!({}));
        fixture.service.bind_local_rpc(&credential, method, &mut params).unwrap();
        assert_eq!(params.unwrap()[field]["tab_id"], tab, "{method}");
    }
}
