use super::model::{choose_owner, surface_placement, surface_tab_row};
use super::{surface_input_refusal, SurfaceTransport, SURFACE_NO_INPUT, SURFACE_NO_TEXT};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{SurfaceBacking, Tab, TabBacking};
use crate::service::tab_transport::TabTransport;

fn surface_backing() -> TabBacking {
    TabBacking::Surface(SurfaceBacking {
        plugin_id: "dash".into(),
        surface_id: "board".into(),
        instance_id: "inst-1".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    })
}

fn tab(configure: impl FnOnce(&mut Tab)) -> Tab {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).expect("initial Tab");
    configure(&mut *tab);
    tab.clone()
}

#[test]
fn surface_owner_defaults_to_an_agent_caller_only() {
    let agent = tab(|tab| tab.agent_type = Some("provider-fixture".into()));
    let shell = tab(|_| {});
    assert_eq!(choose_owner(None, Some(&agent)), Some(agent.id.as_str()));
    assert_eq!(choose_owner(None, Some(&shell)), None);
    assert_eq!(
        choose_owner(Some("explicit"), Some(&agent)),
        Some("explicit")
    );
    assert_eq!(choose_owner(None, None), None);
}

#[test]
fn surface_placement_nests_under_owner_and_joins_its_team() {
    let owner = tab(|tab| {
        tab.team_id = Some("team-1".into());
        tab.depth = 2;
    });
    let placement = surface_placement(Some(&owner));
    assert_eq!(placement.parent_tab_id.as_deref(), Some(owner.id.as_str()));
    assert_eq!(placement.team_id.as_deref(), Some("team-1"));
    assert_eq!(placement.depth, 3);
    assert_eq!(surface_placement(None), Default::default());
}

#[test]
fn surface_tab_row_reports_only_surface_tabs() {
    let surface = tab(|tab| {
        tab.backing = surface_backing();
        tab.title = "Board".into();
        tab.parent_tab_id = Some("owner".into());
    });
    let row = surface_tab_row(&surface, Some("ws".into())).expect("surface row");
    assert_eq!(row.surface_id, "board");
    assert_eq!(row.owner_tab_id.as_deref(), Some("owner"));
    assert!(surface_tab_row(&tab(|_| {}), None).is_none());
}

#[test]
fn surface_tabs_refuse_text_input_with_a_typed_code() {
    let refusal = surface_input_refusal(&surface_backing()).expect("refused");
    assert!(refusal.starts_with(SURFACE_NO_INPUT));
    assert!(surface_input_refusal(&TabBacking::Pty).is_none());
    assert!(surface_input_refusal(&TabBacking::ChatBackend {
        plugin_id: "chat".into(),
        session_id: "s".into(),
    })
    .is_none());

    let transport = SurfaceTransport;
    assert!(transport
        .send("text", true)
        .unwrap_err()
        .starts_with(SURFACE_NO_INPUT));
    assert!(transport
        .send_batch(Vec::new())
        .unwrap_err()
        .starts_with(SURFACE_NO_INPUT));
    assert!(transport
        .read_screen(false)
        .unwrap_err()
        .starts_with(SURFACE_NO_TEXT));
    assert!(transport
        .read_output(None, None, true, false)
        .unwrap_err()
        .starts_with(SURFACE_NO_TEXT));
    assert!(transport.resize(80, 24).is_ok());
    assert!(transport.kill().is_ok());
}
