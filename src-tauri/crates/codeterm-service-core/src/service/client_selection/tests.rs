use super::TAB_FOCUS_REQUESTED_EVENT;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use crate::service::MuxEmitStats;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn two_workspaces() -> (MultiplexerState, String, String, String) {
    let (mut mux, first_tab) = MultiplexerState::new();
    let other = mux.create_workspace("other".into(), None).id;
    let other_tab = mux.create_tab(&other).unwrap();
    (mux, first_tab, other, other_tab)
}

fn focus_events(fixture: &ServiceFixture) -> Vec<String> {
    fixture
        .ui
        .0
        .lock()
        .iter()
        .map(|(event, _)| event.clone())
        .filter(|event| event == "tab-focused" || event == TAB_FOCUS_REQUESTED_EVENT)
        .collect()
}

#[test]
fn a_client_selection_is_recorded_without_reaching_any_other_client() {
    let (mux, _, other, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);
    let requests = || {
        fixture
            .service
            .ext::<Arc<MuxEmitStats>>()
            .requests
            .load(Ordering::Relaxed)
    };
    let before = requests();

    fixture
        .service
        .record_client_selection(Some(&other), Some(&other_tab))
        .unwrap();

    let mux = fixture.service.mux.read();
    assert_eq!(
        mux.get_active_workspace().map(|w| w.id.as_str()),
        Some(other.as_str())
    );
    assert_eq!(
        mux.get_workspace(&other).unwrap().active_tab_id.as_deref(),
        Some(other_tab.as_str())
    );
    assert_eq!(mux.get_focused_tab_id(), Some(other_tab.as_str()));
    drop(mux);
    assert_eq!(
        requests(),
        before,
        "a selection must not broadcast a snapshot"
    );
    assert!(focus_events(&fixture).is_empty());
}

#[test]
fn a_tab_selection_finds_its_own_workspace() {
    let (mux, _, other, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);

    fixture
        .service
        .record_client_selection(None, Some(&other_tab))
        .unwrap();

    let mux = fixture.service.mux.read();
    assert_eq!(
        mux.get_active_workspace().map(|w| w.id.as_str()),
        Some(other.as_str())
    );
    assert_eq!(mux.get_focused_tab_id(), Some(other_tab.as_str()));
}

#[test]
fn an_unknown_tab_selection_is_refused_and_changes_nothing() {
    let (mux, _, _, _) = two_workspaces();
    let fixture = ServiceFixture::new(mux);
    let before = fixture
        .service
        .mux
        .read()
        .get_active_workspace()
        .map(|w| w.id.clone());

    assert!(fixture
        .service
        .record_client_selection(None, Some("missing"))
        .is_err());

    let mux = fixture.service.mux.read();
    assert_eq!(mux.get_active_workspace().map(|w| w.id.clone()), before);
    assert_ne!(mux.get_focused_tab_id(), Some("missing"));
}

#[test]
fn an_explicit_workspace_focus_asks_every_client_to_open_it() {
    let (mux, _, other, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);

    fixture.service.focus_workspace(&other).unwrap();

    let recorded = fixture.ui.0.lock().clone();
    let request = recorded
        .iter()
        .find(|(event, _)| event == TAB_FOCUS_REQUESTED_EVENT)
        .map(|(_, payload)| payload.clone())
        .expect("explicit workspace focus emits its request");
    assert_eq!(request["workspace_id"], other.as_str());
    assert_eq!(request["tab_id"], other_tab.as_str());
}

#[test]
fn an_explicit_focus_request_names_its_tab_and_workspace_for_every_client() {
    let (mux, _, other, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);

    fixture.service.request_tab_focus(&other_tab).unwrap();

    let recorded = fixture.ui.0.lock().clone();
    let request = recorded
        .iter()
        .find(|(event, _)| event == TAB_FOCUS_REQUESTED_EVENT)
        .map(|(_, payload)| payload.clone())
        .expect("explicit focus emits its request");
    assert_eq!(request["tab_id"], other_tab.as_str());
    assert_eq!(request["workspace_id"], other.as_str());
    assert_eq!(
        fixture.service.mux.read().get_focused_tab_id(),
        Some(other_tab.as_str())
    );
}
