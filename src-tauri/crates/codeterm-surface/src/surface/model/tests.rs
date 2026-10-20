use super::*;

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
fn surface_owner_label_follows_agent_binding_or_explicit_request() {
    assert!(owner_label(&board(true), false));
    assert!(owner_label(&board(false), true));
    assert!(!owner_label(&board(false), false));
}
