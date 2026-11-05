use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

fn request(value: Value) -> Result<(String, RuntimeLaunchProfile), String> {
    parse_add(serde_json::from_value(value).unwrap())
}

#[test]
fn add_validates_names_environment_and_secret_references() {
    let valid = json!({"name":"fixture","binary":"fixture-agent","args":["--stdio"],"env":["MODE=test"],"secretEnv":["TOKEN=domios:provider:acp:fixture"],"authEnv":["KEY=domios:provider:acp:key:api-key"]});
    let (_, profile) = request(valid.clone()).unwrap();
    assert_eq!(profile.args, ["--stdio"]);
    assert_eq!(profile.secret_env["TOKEN"], "domios:provider:acp:fixture");
    assert_eq!(profile.auth_env["KEY"].methods, ["api-key"]);
    for (field, bad) in [
        ("name", json!("two words")),
        ("name", json!("../escape")),
        ("name", json!("NUL")),
        ("env", json!(["CODETERM_API_TOKEN=override"])),
        ("env", json!(["codetermOther=override"])),
        ("env", json!(["9KEY=value"])),
        ("env", json!(["MODE=one", "mode=two"])),
        ("secretEnv", json!(["TOKEN=raw-value"])),
        ("secretEnv", json!(["TOKEN=domios:provider:other:key"])),
        ("secretEnv", json!(["MODE=domios:provider:acp:key"])),
        ("authEnv", json!(["KEY=domios:provider:acp:key:"])),
        ("authEnv", json!(["KEY=raw-value:api-key"])),
        ("binary", json!("npx")),
    ] {
        let mut value = valid.clone();
        value[field] = bad;
        assert!(request(value).is_err(), "{field}");
    }
}

#[test]
fn management_requires_owner_or_live_orchestrator_credentials() {
    let (mut mux, ordinary) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&ordinary).unwrap();
    let orchestrator = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&orchestrator).unwrap();
    mux.resolve_or_mint_team_for_parent(&orchestrator).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-fixture".into());
    let ordinary_token = svc
        .frontend_pty_env(Some(&ordinary))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let orchestrator_token = svc
        .frontend_pty_env(Some(&orchestrator))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    assert!(authorize(svc, "owner-fixture").is_ok());
    assert!(authorize(svc, &orchestrator_token).is_ok());
    assert!(authorize(svc, &ordinary_token).is_err());
    assert!(svc
        .bind_local_rpc(
            &ordinary_token,
            "agent.acp",
            &mut Some(json!({"operation":"list"}))
        )
        .is_err());
    assert!(svc
        .bind_local_rpc(
            &orchestrator_token,
            "agent.acp",
            &mut Some(json!({"operation":"list"}))
        )
        .is_ok());
    assert!(execute(
        svc,
        &json!({"operation":"list","authenticated_mesh_peer_id":"fixture-peer"})
    )
    .is_err());
    assert!(authorize(svc, "forged").is_err());
    svc.revoke_tab_credential(&orchestrator);
    assert!(authorize(svc, &orchestrator_token).is_err());
}
