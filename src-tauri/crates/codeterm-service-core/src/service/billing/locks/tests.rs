use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::billing::PLAN_LIMIT_LOCKED;
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
fn unchanged_gate_emits_nothing_and_this_build_enforces_free_before_billing() {
    let (mux, newest) = three_workspaces();
    let fixture = ServiceFixture::new(mux);
    fixture.service.sync_limit_gate();
    assert_eq!(entitlement_events(&fixture), 1);
    assert!(fixture.service.is_locked(LimitKey::Workspaces, &newest));
    fixture.service.sync_limit_gate();
    assert_eq!(entitlement_events(&fixture), 1);
}

fn locked_and_sibling_tabs() -> (MultiplexerState, String, String) {
    let (mut mux, sibling) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let newest = ["a", "b", "c"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .last()
        .cloned()
        .unwrap();
    let locked = mux.create_tab(&newest).unwrap();
    (mux, locked, sibling)
}

fn deliver(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    use crate::service::messenger::{MessageEnvelope, Messenger};
    Messenger::deliver(
        svc,
        tab,
        MessageEnvelope::tab("sender", "sender-tab"),
        "opaque",
    )
    .map(drop)
}

#[test]
fn locked_tab_refuses_every_write_before_the_pty_or_ledger_and_paid_reopens_it() {
    use crate::service::message_gateway::DeliverySeam;
    let (mux, locked, sibling) = locked_and_sibling_tabs();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    let locked_writes = fixture.bind_recorded_agent_recipient(&locked, "locked-s", "locked-n");
    let sibling_writes = fixture.bind_recorded_agent_recipient(&sibling, "sib-s", "sib-n");
    svc.apply_limit_gate(LimitGate::new(compiled_free(), true));

    let refusals = [
        deliver(svc, &locked).unwrap_err(),
        svc.deliver_text(&locked, "opaque").unwrap_err(),
        svc.require_tab_input_allowed(&locked, None).unwrap_err(),
        svc.ensure_tab_pty_for_send(&locked).unwrap_err(),
    ];
    for refusal in &refusals {
        assert!(refusal.starts_with(PLAN_LIMIT_LOCKED), "{refusal}");
    }
    let typed = svc.tab_write_lock_delivery(&locked).unwrap_err();
    assert_eq!(typed.code(), PLAN_LIMIT_LOCKED);
    assert_eq!(typed.status(), axum::http::StatusCode::FORBIDDEN);
    assert!(!typed.bytes_written());
    assert!(typed.surfaces_as_failure(crate::service::delivery_error::SendOrigin::User));
    assert_eq!(
        crate::service::delivery_error::DeliveryError::from_opaque(&locked, &refusals[0]).code(),
        PLAN_LIMIT_LOCKED
    );
    assert!(locked_writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&locked));
    assert!(svc
        .reconcile_user_send_statuses(&locked)
        .unwrap()
        .is_empty());
    assert!(svc.mux.read().get_tab(&locked).is_some());

    deliver(svc, &sibling).unwrap();
    assert_eq!(sibling_writes.texts.lock().as_slice(), ["opaque"]);

    svc.apply_limit_gate(LimitGate::new(paid(), true));
    deliver(svc, &locked).unwrap();
    assert_eq!(locked_writes.texts.lock().len(), 1);
}

#[test]
fn dormant_gate_never_refuses_a_write() {
    let (mux, locked, _) = locked_and_sibling_tabs();
    let fixture = ServiceFixture::new(mux);
    fixture.service.apply_limit_gate(LimitGate::dormant());
    assert!(fixture.service.tab_write_lock(&locked).is_none());
    assert!(fixture.service.tab_write_lock_delivery(&locked).is_ok());
    assert!(fixture
        .service
        .require_tab_input_allowed(&locked, None)
        .is_ok());
}

#[test]
fn service_metadata_writes_on_locked_items_refuse() {
    let (mut mux, newest) = three_workspaces();
    let tab = mux.create_tab(&newest).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.apply_limit_gate(LimitGate::new(compiled_free(), true));
    for result in [
        svc.rename_workspace(&newest, "Renamed"),
        svc.rename_tab(&tab, "Renamed"),
        svc.set_tab_title(&tab, "Renamed"),
        svc.set_tab_description(&tab, "Renamed"),
    ] {
        assert!(result.unwrap_err().starts_with(PLAN_LIMIT_LOCKED));
    }
}
