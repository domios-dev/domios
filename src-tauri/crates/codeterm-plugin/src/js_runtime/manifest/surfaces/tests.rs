use super::super::PluginManifest;
use super::*;

fn manifest(capabilities: serde_json::Value) -> Result<PluginManifest, String> {
    PluginManifest::from_json_str(
        &serde_json::json!({
            "id": "dash",
            "version": "0.1.0",
            "displayName": "Dash",
            "capabilities": capabilities,
        })
        .to_string(),
    )
}

fn surface() -> serde_json::Value {
    serde_json::json!({ "id": "board", "title": "Board", "renderer": "view" })
}

fn with_view(surfaces: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({ "view": true, "surfaces": surfaces })
}

#[test]
fn surface_manifest_parses_view_surface_with_defaults() {
    let parsed = manifest(with_view(vec![surface()])).expect("valid surface");
    assert_eq!(
        parsed.capabilities.surfaces,
        vec![SurfaceDecl {
            id: "board".into(),
            title: "Board".into(),
            renderer: SurfaceRenderer::View,
            agent_bound: false,
            restore: SurfaceRestore::Relaunch,
            runtime: None,
            chrome: None,
            icon: None,
        }]
    );
    assert_eq!(
        parsed
            .capabilities
            .surface("board")
            .map(|s| s.title.as_str()),
        Some("Board")
    );
    assert!(parsed.capabilities.surface("missing").is_none());
}

#[test]
fn surface_manifest_parses_agent_bound_and_every_restore_policy() {
    for (raw, expected) in [
        ("relaunch", SurfaceRestore::Relaunch),
        ("placeholder", SurfaceRestore::Placeholder),
        ("none", SurfaceRestore::Discard),
    ] {
        let mut declared = surface();
        declared["agentBound"] = serde_json::json!(true);
        declared["restore"] = serde_json::json!(raw);
        let parsed = manifest(with_view(vec![declared])).expect("valid restore policy");
        let parsed_surface = &parsed.capabilities.surfaces[0];
        assert!(parsed_surface.agent_bound);
        assert_eq!(parsed_surface.restore, expected);
    }
}

#[test]
fn surface_manifest_round_trips_through_serialization() {
    let mut declared = surface();
    declared["agentBound"] = serde_json::json!(true);
    declared["restore"] = serde_json::json!("none");
    let parsed = manifest(with_view(vec![declared.clone()])).expect("valid surface");
    let value = serde_json::to_value(&parsed.capabilities).expect("serialize");
    assert_eq!(value["surfaces"][0], declared);
}

#[test]
fn surface_manifest_without_surfaces_serializes_unchanged() {
    let parsed = manifest(serde_json::json!({ "view": true })).expect("no surfaces");
    let value = serde_json::to_value(&parsed.capabilities).expect("serialize");
    assert!(value.get("surfaces").is_none());
}

#[test]
fn surface_manifest_view_renderer_requires_view_capability() {
    let error = manifest(serde_json::json!({ "surfaces": [surface()] }))
        .expect_err("view renderer without view capability");
    assert!(error.contains("view"), "got: {error}");
}

#[test]
fn surface_manifest_rejects_multi_word_and_malformed_titles() {
    let too_long = "x".repeat(MAX_SURFACE_TITLE_CHARS + 1);
    for title in ["Two Words", "", " Board", "Tab\u{7}", too_long.as_str()] {
        let mut declared = surface();
        declared["title"] = serde_json::json!(title);
        assert!(
            manifest(with_view(vec![declared])).is_err(),
            "accepted title {title:?}"
        );
    }
}

#[test]
fn surface_manifest_rejects_malformed_entries() {
    let malformed = [
        serde_json::json!({ "id": "Board", "title": "Board", "renderer": "view" }),
        serde_json::json!({ "id": "", "title": "Board", "renderer": "view" }),
        serde_json::json!({ "id": "-board", "title": "Board", "renderer": "view" }),
        serde_json::json!({ "id": "board", "title": "Board", "renderer": "stream" }),
        serde_json::json!({ "id": "board", "title": "Board" }),
        serde_json::json!({ "id": "board", "title": "Board", "renderer": "view", "restore": "later" }),
        serde_json::json!({ "id": "board", "title": "Board", "renderer": "view", "chrome": {} }),
    ];
    for declared in malformed {
        assert!(
            manifest(with_view(vec![declared.clone()])).is_err(),
            "accepted malformed surface {declared}"
        );
    }
}

#[test]
fn surface_manifest_rejects_duplicate_and_excess_surfaces() {
    assert!(manifest(with_view(vec![surface(), surface()])).is_err());
    let excess = (0..=MAX_SURFACES)
        .map(|index| {
            serde_json::json!({ "id": format!("board-{index}"), "title": "Board", "renderer": "view" })
        })
        .collect();
    assert!(manifest(with_view(excess)).is_err());
}

fn stream_surface() -> serde_json::Value {
    serde_json::json!({ "id": "browser", "title": "Browser", "renderer": "stream", "runtime": "browser" })
}

#[test]
fn surface_manifest_stream_surface_needs_no_view_and_names_its_runtime() {
    let mut declared = stream_surface();
    declared["chrome"] = serde_json::json!({ "items": [{ "id": "pick" }] });
    let parsed = manifest(serde_json::json!({ "surfaces": [declared.clone()] }))
        .expect("stream surface without a view");
    let surface = &parsed.capabilities.surfaces[0];
    assert_eq!(surface.renderer, SurfaceRenderer::Stream);
    assert_eq!(surface.stream_runtime(), Some("browser"));
    assert_eq!(
        surface
            .chrome
            .as_ref()
            .and_then(|chrome| chrome.get("items")),
        declared["chrome"].get("items")
    );
    let value = serde_json::to_value(&parsed.capabilities).expect("serialize");
    assert_eq!(
        value["surfaces"][0]["chrome"], declared["chrome"],
        "chrome passes through untouched"
    );
}

#[test]
fn surface_manifest_view_surface_has_no_stream_runtime() {
    let parsed = manifest(with_view(vec![surface()])).expect("valid surface");
    assert_eq!(parsed.capabilities.surfaces[0].stream_runtime(), None);
}

#[test]
fn surface_manifest_rejects_stream_without_valid_runtime_and_view_with_stream_fields() {
    let mut no_runtime = stream_surface();
    no_runtime.as_object_mut().unwrap().remove("runtime");
    let mut bad_runtime = stream_surface();
    bad_runtime["runtime"] = serde_json::json!("Browser Engine");
    let mut array_chrome = stream_surface();
    array_chrome["chrome"] = serde_json::json!([]);
    for declared in [no_runtime, bad_runtime, array_chrome] {
        assert!(
            manifest(serde_json::json!({ "surfaces": [declared.clone()] })).is_err(),
            "accepted {declared}"
        );
    }
    let mut view_with_runtime = surface();
    view_with_runtime["runtime"] = serde_json::json!("browser");
    assert!(manifest(with_view(vec![view_with_runtime])).is_err());
}

#[test]
fn surface_manifest_keeps_a_kebab_icon_and_round_trips_it() {
    for icon in ["globe", "layout-dashboard", "app-window-2"] {
        let mut declared = surface();
        declared["icon"] = serde_json::json!(icon);
        let parsed = manifest(with_view(vec![declared])).expect("valid icon");
        assert_eq!(parsed.capabilities.surfaces[0].icon.as_deref(), Some(icon));
        let value = serde_json::to_value(&parsed.capabilities).expect("serialize");
        assert_eq!(value["surfaces"][0]["icon"], icon);
    }
}

#[test]
fn surface_manifest_drops_an_invalid_icon_and_still_loads() {
    let too_long = "a".repeat(MAX_SURFACE_ICON_BYTES + 1);
    for icon in [
        "Globe",
        "",
        "-globe",
        "globe-",
        "two--dashes",
        "globe icon",
        "../x",
        "https://x/icon.svg",
        too_long.as_str(),
    ] {
        let mut declared = surface();
        declared["icon"] = serde_json::json!(icon);
        let parsed = manifest(with_view(vec![declared])).expect("an icon never fails the plugin");
        assert_eq!(parsed.capabilities.surfaces[0].icon, None, "kept {icon:?}");
    }
    let mut not_a_string = surface();
    not_a_string["icon"] = serde_json::json!(7);
    assert!(manifest(with_view(vec![not_a_string])).is_err());
}
