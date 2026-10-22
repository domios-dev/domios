use super::retarget_only;
use serde_json::json;

#[test]
fn curator_retarget_preserves_commands_thresholds_and_steps() {
    let before = json!({"schema_version":2,"kind":"pipeline","name":"retarget","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"read","run":{"lit":"opaque-command"}},{"id":"send","uses":"filter","with":{"pattern":{"lit":"opaque-pattern"}}}],"result":{"to":"tab:old"}});
    let mut after = before.clone();
    after["result"]["to"] = json!("tab:new");
    assert!(retarget_only(&before, &after));
    after["steps"][0]["run"] = json!({"lit":"changed-command"});
    assert!(!retarget_only(&before, &after));
    after = before.clone();
    after["steps"][1]["with"]["pattern"] = json!({"lit":"changed-pattern"});
    assert!(!retarget_only(&before, &after));
}

#[test]
fn curator_native_retarget_changes_only_declared_target_slots() {
    let before = json!({"schema_version":2,"kind":"watch","name":"retarget","steps":[{"id":"source","uses":"observe","with":{"kind":{"lit":"tab"},"id":{"lit":"old"}}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"]},{"id":"effect","uses":"send","with":{"target":{"lit":"old"}}}]});
    let mut after = before.clone();
    after["steps"][0]["with"]["id"] = json!({"lit":"new"});
    after["steps"][2]["with"]["target"] = json!({"lit":"new"});
    assert!(retarget_only(&before, &after));
    after["steps"][0]["id"] = json!("new-node");
    assert!(!retarget_only(&before, &after));
    let before = json!({"schema_version":2,"kind":"pipeline","name":"opaque literal","steps":[{"id":"node","uses":"filter","with":{"pattern":{"lit":{"target":"old"}}}}]});
    let mut after = before.clone();
    after["steps"][0]["with"]["pattern"]["lit"]["target"] = json!("new");
    assert!(!retarget_only(&before, &after));
}
