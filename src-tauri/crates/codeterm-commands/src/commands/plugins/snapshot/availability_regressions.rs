use super::*;

#[test]
fn same_versions_and_core_plugins_never_offer_actions() {
    let mut plugins = serde_json::json!([
        {"id":"lark", "version":"0.1.3", "system":false, "source":{"kind":"channel","channel":"store"}, "update":{"available":true}},
        {"id":"core", "version":"0.1.2", "system":true, "state":"bundled", "source":{"kind":"system_channel","channel":"official"}, "update":{"available":true}}
    ]);
    let diff = serde_json::json!([
        {"id":"lark","channel":"store","remote_version":"0.1.3","payload_version":"0.1.3","compatible":true,"drift":"hash_drift"},
        {"id":"core","channel":"official","remote_version":"0.1.3","payload_version":"0.1.3","compatible":true,"drift":"update_available"}
    ]);
    reconcile_update_availability(&mut plugins, &diff);
    assert_eq!(plugins[0]["update"]["available"], false);
    assert_eq!(plugins[0]["update"]["availability"]["state"], "up_to_date");
    assert_eq!(plugins[1]["update"]["available"], false);
    assert_eq!(plugins[1]["update"]["availability"]["state"], "core_silent");
}
