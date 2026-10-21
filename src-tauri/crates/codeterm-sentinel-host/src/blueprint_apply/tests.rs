use super::retarget_only;
use serde_json::json;

#[test]
fn curator_retarget_preserves_commands_thresholds_and_steps() {
    let before = json!({"create":{"probe":"opaque-command","observe":{"tab":"old"},"threshold":3},"spec":{"result":{"to":"old"},"steps":[{"command":"opaque-step"}]}});
    let mut after = before.clone();
    after["create"]["observe"]["tab"] = json!("new");
    after["spec"]["result"]["to"] = json!("new");
    assert!(retarget_only(&before, &after));
    for path in [vec!["create", "probe"], vec!["create", "threshold"]] {
        let mut changed = after.clone();
        changed[path[0]][path[1]] = json!("changed");
        assert!(!retarget_only(&before, &changed));
    }
    after["spec"]["steps"][0]["command"] = json!("new-command");
    assert!(!retarget_only(&before, &after));
}
