// WHAT: confirmed chat closure through the pinned adapter and runtime process.
// WHY: a removed session binding or best-effort close is not an exit acknowledgement.
// ORACLE: receipts follow exact-session adapter success and the host process exit event.
use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn runtime_fixture() -> (ServiceFixture, String, crate::service::agent_runtime::engine::test_support::LiveRuntime) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("thread".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.backing = TabBacking::ChatBackend { plugin_id: "fixture".into(), session_id: "thread".into() };
    let prepared = tab.clone();
    let fixture = ServiceFixture::new(mux);
    let live = engine::test_support::live_runtime(&prepared);
    (fixture, id, live)
}

#[test]
fn confirmed_runtime_close_waits_for_the_pinned_process_exit() {
    let (fixture, id, _live) = runtime_fixture();
    let session = engine::for_tab(&id).unwrap();
    let process = session.process.lock().clone().unwrap();
    let receipt = fixture.service.chatbackend_close_confirmed(&id).unwrap();
    assert_eq!(receipt.tab_id, id);
    assert_eq!(receipt.session_id, "thread");
    assert!(matches!(*process.ended.borrow(), Some(RuntimeFailure::ProcessExited { .. })));
    assert!(engine::for_tab(&id).is_none());
}

#[test]
fn changed_runtime_nonce_refuses_without_stopping_another_session() {
    let (fixture, id, _live) = runtime_fixture();
    let session = engine::for_tab(&id).unwrap();
    let process = session.process.lock().clone().unwrap();
    fixture.service.mux.write().get_tab_mut(&id).unwrap().session_binding_nonce = Some("foreign".into());
    assert!(fixture.service.chatbackend_close_confirmed(&id).is_err());
    assert!(process.ended.borrow().is_none());
    assert!(engine::for_tab(&id).is_some());
}

#[test]
fn missing_runtime_and_adapter_cannot_mint_a_receipt() {
    let (fixture, id, live) = runtime_fixture();
    drop(live);
    assert!(fixture.service.chatbackend_close_confirmed(&id).is_err());
}

#[test]
fn close_lock_deadline_refuses_before_entering_the_transition() {
    let guards = TabReopenGuards::default();
    let lock = guards.lock_for("tab");
    let _held = lock.lock();
    let result = guards.serialized_until::<()>("tab", Instant::now(), || panic!("close lock is held"));
    assert!(result.is_err());
}

#[test]
fn stale_pinned_runtime_refuses_the_new_runtime() {
    let (fixture, id, _old_live) = runtime_fixture();
    let old = engine::for_tab(&id).unwrap();
    let old_process = old.process.lock().clone().unwrap();
    let prepared = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let _new_live = engine::test_support::live_runtime(&prepared);
    let new_process = engine::for_tab(&id).unwrap().process.lock().clone().unwrap();
    assert!(engine::stop_confirmed_blocking(&fixture.service, &id, old).is_err());
    assert!(new_process.ended.borrow().is_none());
    crate::async_runtime::block_on(old_process.finish()).unwrap();
}

#[test]
fn plugin_close_receipt_is_required_before_removing_the_bound_adapter() {
    for (source, accepted) in [
        ("module.exports.default = { closeSession(id) { return {sessionId:id, processExited:true}; } };", true),
        ("module.exports.default = { closeSession() { throw new Error('close failed'); } };", false),
        ("module.exports.default = { closeSession() { return {sessionId:'foreign', processExited:true}; } };", false),
        ("module.exports.default = { closeSession(id) { return {sessionId:id, processExited:false}; } };", false),
    ] {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.backing = TabBacking::ChatBackend { plugin_id: "plugin-fixture".into(), session_id: "thread".into() };
        tab.agent_session_id = Some("thread".into());
        tab.session_binding_nonce = Some("nonce".into());
        let fixture = ServiceFixture::new(mux);
        let vm = codeterm_plugin::js_runtime::PluginVm::from_source("plugin-fixture", source.into()).unwrap();
        let adapter = codeterm_plugin::js_runtime::JsChatBackend::new(Arc::new(vm), "plugin-fixture");
        let bound = Arc::new(super::super::liveness::BoundSession::new(id.clone(), Arc::new(adapter)));
        let key = ("plugin-fixture".into(), "thread".into());
        let guards = fixture.service.ext::<Arc<TabReopenGuards>>();
        guards.sessions.lock().insert(key.clone(), bound.clone());
        let result = fixture.service.chatbackend_close_confirmed(&id);
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(guards.sessions.lock().contains_key(&key), !accepted);
        assert!(bound.current().is_err());
        assert!(fixture.service.mux.read().get_tab(&id).is_some());
    }
}
