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
