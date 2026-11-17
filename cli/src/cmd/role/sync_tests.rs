use super::render_sync;
use serde_json::json;

#[test]
fn role_sync_preview_renders_hashes_source_effect_and_opaque_diff() {
    let value = json!({"team_id":"team_a","role":"curator","current_hash":"old",
        "replacement_hash":"new","source":{"name":"opaque-source","version":"7"},
        "diff":"opaque-diff\n-second\n+third\n","takes_effect":"opaque-effect"});
    let rendered = render_sync(&value);
    for fragment in [
        "team_a",
        "curator",
        "opaque-source@7",
        "old",
        "new",
        "opaque-effect",
        "opaque-diff\n-second\n+third\n",
    ] {
        assert!(rendered.contains(fragment));
    }
    let waiting = render_sync(
        &json!({"preview":value,"state":"awaiting_owner","notification_id":"notification-a"}),
    );
    assert!(waiting.contains("notification-a"));
    assert!(waiting.contains("awaiting_owner"));
}
