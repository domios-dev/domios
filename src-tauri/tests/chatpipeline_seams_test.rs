#[path = "../crates/codeterm-service-core/src/test_support.rs"]
mod test_support;

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[test]
fn outbound_stages_only_run_at_user_facing_send_seams() {
    let rest = include_str!("../crates/codeterm-api/src/tab_routes/send/preparation.rs");
    let composer = include_str!("../crates/codeterm-service-core/src/service/agent_pad.rs");
    let chat = include_str!("../crates/codeterm-service-core/src/service/tab_transport_chat.rs");
    let batch = include_str!("../crates/codeterm-service-core/src/service/pty_io/send/mod.rs");
    let messenger = include_str!("../crates/codeterm-service-core/src/service/messenger.rs");
    let bridge = include_str!("../crates/codeterm-bridge/src/bridge/manager/mod.rs");

    assert_eq!(occurrences(rest, "run_outbound_stages("), 1);
    assert_eq!(occurrences(composer, "run_outbound_stages("), 1);
    assert_eq!(occurrences(chat, "run_outbound_stages("), 0);
    assert_eq!(occurrences(batch, "run_outbound_stages("), 0);
    assert_eq!(occurrences(messenger, "run_outbound_stages("), 0);
    assert_eq!(occurrences(bridge, "run_outbound_stages("), 0);
}

fn outbound_calls_in(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                outbound_calls_in(&path)
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                occurrences(
                    &std::fs::read_to_string(path).unwrap(),
                    ".run_outbound_stages(",
                )
            } else {
                0
            }
        })
        .sum()
}

#[test]
fn outbound_stages_have_no_additional_crate_callers() {
    let crates = test_support::fixture_manifest_dir().join("crates");
    assert_eq!(outbound_calls_in(&crates), 2);
}
