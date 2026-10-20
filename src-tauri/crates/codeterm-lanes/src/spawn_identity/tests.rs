use super::*;
#[test]
fn resolve_spawn_workspace_prefers_explicit_workspace_over_parent() {
    let workspace_of = |tab: &str| match tab {
        "parent-tab" => Some("ws-parent".to_string()),
        "caller-tab" => Some("ws-caller".to_string()),
        _ => None,
    };
    let resolved = resolve_spawn_workspace_id(
        Some("ws-explicit".into()),
        Some("parent-tab"),
        Some("caller-tab"),
        workspace_of,
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(resolved, "ws-explicit");

    let from_parent = resolve_spawn_workspace_id(
        None,
        Some("parent-tab"),
        Some("caller-tab"),
        workspace_of,
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(from_parent, "ws-parent");
}

/// The focused workspace is what agents kept landing in: without a parent, the
/// spawn belongs to the caller's own workspace, not to the user's UI selection.
#[test]
fn resolve_spawn_workspace_prefers_caller_tab_over_focused_workspace() {
    let workspace_of = |tab: &str| match tab {
        "caller-tab" => Some("ws-caller".to_string()),
        _ => None,
    };
    let from_caller = resolve_spawn_workspace_id(
        None,
        None,
        Some("caller-tab"),
        workspace_of,
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(from_caller, "ws-caller");

    let unknown_caller = resolve_spawn_workspace_id(
        None,
        None,
        Some("ghost-tab"),
        workspace_of,
        Some("ws-active".into()),
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(unknown_caller, "ws-active");

    let no_identity = resolve_spawn_workspace_id(
        None,
        None,
        None,
        workspace_of,
        None,
        Some("ws-fallback".into()),
    )
    .unwrap();
    assert_eq!(no_identity, "ws-fallback");
}
