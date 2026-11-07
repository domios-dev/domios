use super::*;
use crate::agent::test_provider::MockProvider;
use crate::service::test_fixture::ServiceFixture;

fn fixture() -> ServiceFixture {
    let fixture = ServiceFixture::new(crate::multiplexer::state::MultiplexerState::new().0);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_chat_ui("opaque-capable")
            .with_auto_approve_flags(vec!["--opaque-bypass".into()]),
    ));
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::with_chat_ui("opaque-incapable")));
    fixture
}

#[test]
fn permission_bypass_requires_request_and_provider_capability() {
    let fixture = fixture();
    let registry = &fixture.service.agent_registry;
    assert!(authorize_permission_bypass(registry, "opaque-capable", true).is_some());
    assert!(authorize_permission_bypass(registry, "opaque-capable", false).is_none());
    assert!(authorize_permission_bypass(registry, "opaque-incapable", true).is_none());
    assert!(authorize_permission_bypass(registry, "missing-provider", true).is_none());
}

#[test]
fn runtime_host_approvals_authorize_a_flagless_full_access_launch() {
    let fixture = fixture();
    let runtime = |host_approvals: bool| {
        serde_json::from_value::<codeterm_plugin::agent_runtime::AgentRuntimeCapability>(
            serde_json::json!({
                "runtime":"stdio-jsonrpc", "command":{"binary":"opaque","args":[]},
                "hostApprovals":host_approvals,
                "requestMethods":{"initialize":["initialize"]},"notificationMethods":[]
            }),
        )
        .unwrap()
    };
    for (id, declared) in [("opaque-host-approvals", true), ("opaque-runtime", false)] {
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::with_chat_ui(id).with_agent_runtime(runtime(declared)),
        ));
    }
    let registry = &fixture.service.agent_registry;
    assert!(authorize_permission_bypass(registry, "opaque-host-approvals", true).is_some());
    assert!(authorize_permission_bypass(registry, "opaque-host-approvals", false).is_none());
    assert!(authorize_permission_bypass(registry, "opaque-runtime", true).is_none());
}

#[test]
fn authorization_is_provider_bound() {
    let fixture = fixture();
    let authorization =
        authorize_permission_bypass(&fixture.service.agent_registry, "opaque-capable", true)
            .unwrap();
    assert!(authorization.authorizes("opaque-capable"));
    assert!(!authorization.authorizes("different-provider"));
}

#[test]
fn retired_global_choice_does_not_gate_default_access() {
    let fixture = fixture();
    for choice in [None, Some(false), Some(true)] {
        fixture.set_agent_full_access_choice(choice);
        assert!(configured_permission_bypass_for(
            &fixture.service,
            &fixture.service.agent_registry,
            "opaque-capable",
        )
        .unwrap()
        .is_some());
        assert!(
            serde_json::to_value(&fixture.service.configuration()).unwrap()["agent"]
                .get("full_access")
                .is_none()
        );
    }
}

#[test]
fn recorded_permission_policy_is_not_widened_for_any_role() {
    use crate::multiplexer::types::AgentRole;
    let fixture = fixture();
    let tab_id = fixture
        .service
        .mux
        .read()
        .all_tabs()
        .next()
        .unwrap()
        .id
        .clone();
    for role in [AgentRole::Manager, AgentRole::Worker] {
        for choice in [None, Some(false), Some(true)] {
            fixture.set_agent_full_access_choice(choice);
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.role = Some(role);
            tab.restorable_launch.captured = true;
            tab.restorable_launch.permission_bypass_requested = false;
            drop(mux);
            assert!(
                permission_bypass_for_tab(&fixture.service, "opaque-capable", &tab_id,)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
