fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[test]
fn outbound_stages_only_run_at_user_facing_send_seams() {
    let rest = include_str!("../src/api/pane_routes/send.rs");
    let composer = include_str!("../src/commands/pane.rs");
    let chat = include_str!("../src/service/pane_transport_chat.rs");
    let batch = include_str!("../src/service/pty_io/send/mod.rs");
    let messenger = include_str!("../src/service/messenger.rs");
    let bridge = include_str!("../crates/codeterm-bridge/src/bridge/manager/mod.rs");

    assert_eq!(occurrences(rest, "run_outbound_stages("), 2);
    assert_eq!(occurrences(composer, "run_outbound_stages("), 1);
    assert_eq!(occurrences(chat, "run_outbound_stages("), 0);
    assert_eq!(occurrences(batch, "run_outbound_stages("), 0);
    assert_eq!(occurrences(messenger, "run_outbound_stages("), 0);
    assert_eq!(occurrences(bridge, "run_outbound_stages("), 0);
}
