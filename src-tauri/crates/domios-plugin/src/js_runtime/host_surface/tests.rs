use super::*;

#[test]
fn surface_agent_caller_scope_nests_and_restores() {
    assert_eq!(current_agent_caller(), None);
    {
        let _outer = AgentCallerScope::enter("tab-1");
        assert_eq!(current_agent_caller().as_deref(), Some("tab-1"));
        {
            let _inner = AgentCallerScope::enter("worker-2");
            assert_eq!(current_agent_caller().as_deref(), Some("worker-2"));
        }
        assert_eq!(current_agent_caller().as_deref(), Some("tab-1"));
        let _blank = AgentCallerScope::enter("  ");
        assert_eq!(current_agent_caller(), None);
    }
    assert_eq!(current_agent_caller(), None);
}

#[test]
fn surface_call_refuses_without_grant_or_caller() {
    let confined = PluginSandbox::restricted(vec![], vec![]);
    let denied: serde_json::Value =
        serde_json::from_str(&surface_call(&confined, "status", "[]")).unwrap();
    assert_eq!(denied["error"], SURFACE_DENIED);
    let system = PluginSandbox::unrestricted();
    let no_caller: serde_json::Value =
        serde_json::from_str(&surface_call(&system, "status", "[]")).unwrap();
    assert_eq!(no_caller["error"], NO_AGENT_CALLER);
    let _scope = AgentCallerScope::enter("tab-1");
    let no_service: serde_json::Value =
        serde_json::from_str(&surface_call(&system, "status", "[]")).unwrap();
    assert_eq!(no_service["error"], "host service unavailable");
}
