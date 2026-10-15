use codeterm_service_core::notification::authority;
const ASKING_TAB: &str = "tab_the_one_that_asked";

#[test]
fn channel_ask_authority_refuses_every_unattended_surface_change() {
    // The config seam an agent's own CLI reaches: a keyed write is refused,
    // and a whole-object write from inside a tab is refused too.
    let mut surface_partial = serde_json::Map::new();
    surface_partial.insert(
        authority::SURFACE_ROOT.to_string(),
        serde_json::json!({"desktop": true}),
    );
    let empty = serde_json::Map::new();
    assert!(
        crate::config_routes::admit_config_write(None, Some("notifications.desktop"), &empty)
            .is_err(),
        "the keyed write is the shape an agent's CLI makes"
    );
    assert!(
        crate::config_routes::admit_config_write(Some(ASKING_TAB), None, &surface_partial).is_err(),
        "a whole-object write from inside a tab is still nobody tapping anything"
    );
    assert!(
        crate::config_routes::admit_config_write(None, Some("terminal.font_size"), &empty).is_ok(),
        "the rule guards the surface subtree and nothing else"
    );
}
