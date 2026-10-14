use super::*;

#[test]
fn filled_composer_after_angle_bracket_history_is_not_a_delivery_modal() {
    let (native, js) = pair();
    let snapshot = |screen_text: String| crate::tab_state::TabStateSnapshot {
        tab_id: "fixture-tab".into(),
        agent_type: js.id().into(),
        session_id: Some("fixture-session".into()),
        cwd: Some("/fixture".into()),
        screen_text,
    };
    let parked = snapshot(provider_fixture(
        js.id(),
        "tab-state/filled-composer-numbered-history.txt",
    ));
    assert!(native.parse_prompt(&parked.screen_text).is_none());
    assert!(js.parse_prompt(&parked.screen_text).is_none());
    assert!(!native.delivery_modal_visible(&parked));
    assert!(!js.delivery_modal_visible(&parked));

    let live = "Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel · Tab to amend\n";
    assert!(native.parse_prompt(live).is_some());
    assert!(js.parse_prompt(live).is_some());
    let filled = format!(
        "{live}❯ <domios type=\"attachment\" kind=\"voice\" path=\"voice-note-a\" label=\"voice\" />\n"
    );
    assert!(native.parse_prompt(&filled).is_none());
    assert!(js.parse_prompt(&filled).is_none());
    let filled_snapshot = snapshot(filled);
    assert!(!native.delivery_modal_visible(&filled_snapshot));
    assert!(!js.delivery_modal_visible(&filled_snapshot));
}
