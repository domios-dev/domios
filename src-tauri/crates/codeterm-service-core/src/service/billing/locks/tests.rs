use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_entitlements::{compiled_free, Authority, EffectiveAccess, Perks, Scope, ScopedLimit};

fn paid() -> EffectiveAccess {
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

fn three_workspaces() -> (MultiplexerState, String) {
    let (mut mux, _) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let newest = ["a", "b", "c"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .last()
        .cloned()
        .unwrap();
    (mux, newest)
}

fn entitlement_events(fixture: &ServiceFixture) -> usize {
    fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(event, _)| event == ENTITLEMENT_CHANGED_EVENT)
        .count()
}

#[test]
fn lock_state_follows_free_paid_free_without_restart() {
    let (mux, newest) = three_workspaces();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    assert!(!svc.is_locked(LimitKey::Workspaces, &newest));
    let free = LimitGate::new(compiled_free(), true);
    let states: Vec<bool> = [free.clone(), LimitGate::new(paid(), true), free]
        .into_iter()
        .map(|gate| {
            svc.apply_limit_gate(gate);
            svc.is_locked(LimitKey::Workspaces, &newest)
        })
        .collect();
    assert_eq!(states, [true, false, true]);
    assert_eq!(entitlement_events(&fixture), 3);
    assert_eq!(svc.mux.read().list_workspaces().len(), 4);
}

#[test]
fn unchanged_gate_emits_nothing_and_this_build_stays_dormant() {
    let (mux, newest) = three_workspaces();
    let fixture = ServiceFixture::new(mux);
    fixture.service.sync_limit_gate();
    assert_eq!(entitlement_events(&fixture), 0);
    assert!(fixture.service.locks().workspaces.is_empty());
    assert!(!fixture.service.is_locked(LimitKey::Workspaces, &newest));
}
