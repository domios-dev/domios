use super::*;

#[test]
fn replayable_output_covers_aggregate_per_tab_and_relay_only() {
    for path in [
        "/api/output/stream?since=x",
        "/api/tabs/own/output/stream",
        "/api/mesh/relay-stream?peer=peer&path=%2Fapi%2Foutput%2Fstream",
        "/api/mesh/relay-stream?peer=peer&path=%2Fapi%2Ftabs%2Fown%2Foutput%2Fstream",
    ] {
        assert!(replayable_output(&path.parse().unwrap()), "{path}");
    }
    for path in [
        "/api/events/stream",
        "/api/surfaces/own/stream",
        "/api/mesh/relay-stream?path=%2Fapi%2Fevents%2Fstream",
        "/api/mesh/relay-stream?path=http%3A%2F%2Fother%2Fapi%2Foutput%2Fstream",
    ] {
        assert!(!replayable_output(&path.parse().unwrap()), "{path}");
    }
}
