use codeterm_plugin::surfaces::{SurfaceDecl, SurfaceRenderer, SurfaceRestore};

use super::model::{
    choose_owner, owner_label, parse_surface_target, resolve_surface_decl, surface_placement,
    surface_renderable, surface_tab_row, DeclaredSurfaceRow, PluginSurfaceFacts, SurfaceError,
    SurfaceTarget,
};
use super::{surface_input_refusal, SurfaceTransport, SURFACE_NO_INPUT, SURFACE_NO_TEXT};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{SurfaceBacking, Tab, TabBacking};
use crate::service::tab_transport::TabTransport;

fn board(agent_bound: bool) -> SurfaceDecl {
    SurfaceDecl {
        id: "board".into(),
        title: "Board".into(),
        renderer: SurfaceRenderer::View,
        agent_bound,
        restore: SurfaceRestore::Relaunch,
        runtime: None,
        chrome: None,
        icon: None,
    }
}

fn browser_stream() -> SurfaceDecl {
    SurfaceDecl {
        id: "board".into(),
        title: "Browser".into(),
        renderer: SurfaceRenderer::Stream,
        agent_bound: true,
        restore: SurfaceRestore::Placeholder,
        runtime: Some("browser".into()),
        chrome: Some(serde_json::json!({ "items": [] })),
        icon: Some("globe".into()),
    }
}

fn target() -> SurfaceTarget {
    parse_surface_target("dash:board").expect("valid target")
}

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
fn surface_target_parses_plugin_and_surface_ids() {
    assert_eq!(
        target(),
        SurfaceTarget {
            plugin_id: "dash".into(),
            surface_id: "board".into(),
        }
    );
    for raw in ["dash", ":board", "dash:", "dash:board:extra", ""] {
        assert_eq!(
            parse_surface_target(raw),
            Err(SurfaceError::InvalidTarget(raw.into())),
            "accepted {raw:?}"
        );
    }
}

#[test]
fn surface_open_validation_needs_an_enabled_view_plugin_declaring_the_surface() {
    let declared = |view_registered| PluginSurfaceFacts {
        declared: Some(vec![board(true)]),
        view_registered,
        runtimes: Vec::new(),
    };
    assert_eq!(
        resolve_surface_decl(&target(), &declared(true)),
        Ok(board(true))
    );
    assert_eq!(
        resolve_surface_decl(&target(), &declared(false)),
        Err(SurfaceError::ViewUnavailable("dash".into()))
    );
    assert_eq!(
        resolve_surface_decl(&target(), &PluginSurfaceFacts::default()),
        Err(SurfaceError::PluginUnavailable("dash".into()))
    );
    let view_without_surfaces = PluginSurfaceFacts {
        declared: None,
        view_registered: true,
        runtimes: Vec::new(),
    };
    let undeclared = parse_surface_target("dash:other").unwrap();
    let error = resolve_surface_decl(&target(), &view_without_surfaces).unwrap_err();
    assert_eq!(error.code(), "surface_not_declared");
    let error = resolve_surface_decl(&undeclared, &declared(true)).unwrap_err();
    assert_eq!(error.code(), "surface_not_declared");
}

#[test]
fn surface_open_of_a_stream_surface_needs_its_runtime_not_a_view() {
    let facts = |runtimes: Vec<String>| PluginSurfaceFacts {
        declared: Some(vec![browser_stream()]),
        view_registered: false,
        runtimes,
    };
    assert_eq!(
        resolve_surface_decl(&target(), &facts(vec!["browser".into()])),
        Ok(browser_stream())
    );
    assert_eq!(
        resolve_surface_decl(&target(), &facts(Vec::new())),
        Err(SurfaceError::RuntimeUnavailable("browser".into()))
    );
    assert!(surface_renderable(
        &browser_stream(),
        false,
        &["browser".into()]
    ));
    assert!(!surface_renderable(
        &board(false),
        false,
        &["browser".into()]
    ));
    let row = DeclaredSurfaceRow::new("web", &browser_stream());
    let value = serde_json::to_value(&row).unwrap();
    assert_eq!(value["renderer"], "stream");
    assert_eq!(value["runtime"], "browser");
    assert_eq!(value["chrome"], serde_json::json!({ "items": [] }));
    assert_eq!(value["icon"], "globe");
    let board_row = serde_json::to_value(DeclaredSurfaceRow::new("dash", &board(false))).unwrap();
    assert!(board_row.get("runtime").is_none());
    assert!(board_row.get("icon").is_none());
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
fn surface_owner_label_follows_agent_binding_or_explicit_request() {
    assert!(owner_label(&board(true), false));
    assert!(owner_label(&board(false), true));
    assert!(!owner_label(&board(false), false));
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
