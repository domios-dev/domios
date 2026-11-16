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
        .filter(|event| event == "tab-focused" || event == "tab-focus-requested")
        .collect()
}

fn snapshot_requests(fixture: &ServiceFixture) -> u64 {
    fixture
        .service
        .ext::<Arc<MuxEmitStats>>()
        .requests
        .load(Ordering::Relaxed)
}

/// The last selection any client made: active workspace, its active tab, focused tab.
fn last_selection(
    fixture: &ServiceFixture,
) -> (Option<String>, Vec<Option<String>>, Option<String>) {
    let mux = fixture.service.mux.read();
    (
        mux.get_active_workspace().map(|w| w.id.clone()),
        mux.snapshot()
            .workspaces
            .into_iter()
            .map(|workspace| workspace.active_tab_id)
            .collect(),
        mux.get_focused_tab_id().map(str::to_string),
    )
}

fn highlighted(fixture: &ServiceFixture, tab_id: &str) -> bool {
    fixture
        .service
        .mux
        .read()
        .get_tab(tab_id)
        .unwrap()
        .focus_requested
}

#[test]
fn a_client_selection_is_recorded_without_reaching_any_other_client() {
    let (mut mux, _, other, other_tab) = two_workspaces();
    mux.get_tab_mut(&other_tab).unwrap().pty_id = Some("already-open-pty".into());
    let fixture = ServiceFixture::new(mux);
    let before = snapshot_requests(&fixture);

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
        snapshot_requests(&fixture),
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
fn a_tab_focus_request_highlights_the_tab_and_moves_no_client() {
    let (mux, first_tab, _, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .record_client_selection(None, Some(&first_tab))
        .unwrap();
    let selection = last_selection(&fixture);
    let before = snapshot_requests(&fixture);

    fixture.service.request_tab_focus(&other_tab).unwrap();

    assert!(highlighted(&fixture, &other_tab));
    assert_eq!(last_selection(&fixture), selection);
    assert!(focus_events(&fixture).is_empty());
    assert!(
        snapshot_requests(&fixture) > before,
        "clients learn of the highlight from the snapshot"
    );
}

#[test]
fn a_workspace_focus_request_highlights_its_last_selected_tab_and_moves_no_client() {
    let (mux, first_tab, other, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .record_client_selection(None, Some(&first_tab))
        .unwrap();
    let selection = last_selection(&fixture);

    fixture.service.request_workspace_focus(&other).unwrap();

    assert!(highlighted(&fixture, &other_tab));
    assert!(!highlighted(&fixture, &first_tab));
    assert_eq!(last_selection(&fixture), selection);
    assert!(focus_events(&fixture).is_empty());
}

#[test]
fn focus_requests_for_unknown_targets_are_refused() {
    let (mux, _, _, _) = two_workspaces();
    let fixture = ServiceFixture::new(mux);

    assert!(fixture.service.request_tab_focus("missing").is_err());
    assert!(fixture.service.request_workspace_focus("missing").is_err());
}

#[test]
fn selecting_a_highlighted_tab_in_any_client_clears_the_highlight() {
    let (mux, _, _, other_tab) = two_workspaces();
    let fixture = ServiceFixture::new(mux);
    fixture.service.request_tab_focus(&other_tab).unwrap();
    let before = snapshot_requests(&fixture);

    fixture
        .service
        .record_client_selection(None, Some(&other_tab))
        .unwrap();

    assert!(!highlighted(&fixture, &other_tab));
    assert!(
        snapshot_requests(&fixture) > before,
        "other clients drop the highlight from the snapshot"
    );
}
