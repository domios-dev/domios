use crate::multiplexer::state::MultiplexerState;
use crate::service::billing::LimitGate;
use codeterm_entitlements::{compiled_free, Authority, EffectiveAccess, Perks, Scope, ScopedLimit};

pub fn paid() -> EffectiveAccess {
    let mut access = compiled_free();
    access.authority = Authority::Billing;
    access.perks = Perks::default();
    for limit in [
        &mut access.limits.workspaces,
        &mut access.limits.teams,
        &mut access.limits.agent_tabs,
    ] {
        *limit = ScopedLimit {
            max: None,
            scope: Scope::Installation,
        };
    }
    access
}

pub fn three_workspaces() -> (MultiplexerState, String) {
    let (mut mux, _) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let newest = ["a", "b", "c"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .last()
        .cloned()
        .unwrap();
    mux.set_limit_gate(LimitGate::dormant());
    (mux, newest)
}
