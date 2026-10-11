use super::tests::{TestRegistrar, set_mtime, test_caps, write};
use super::*;
use crate::local_recording::{
    LocalInputSubscription, LocalRecordingTrigger, SelectedDevices, SubscriptionTeardown,
};
use std::sync::atomic::Ordering;

fn manifest(id: &str) -> String {
    serde_json::json!({
        "id": id, "displayName": "Fixture", "version": "0.1.0",
        "hostApi": crate::js_runtime::host_api::HOST_API_VERSION, "entry": "plugin.js",
        "capabilities": { "localRecordingTrigger": { "vendorId": 1, "productId": 2, "usagePage": 3, "usage": 4 },
            "tabMenu": [{ "id": "receive", "label": "Receive here", "action": "receiverTarget" }] },
        "permissions": { "local_recording": true }
    }).to_string()
}

struct ReadySubscription;

impl LocalInputSubscription for ReadySubscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Ok(())).unwrap();
        SubscriptionTeardown::new(receiver)
    }
}

#[test]
fn lark_recording_contract_native_js_declaration_and_reload_grant_parity() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("trigger");
    write(&dir.join("plugin.json"), &manifest("trigger"));
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    let caps = test_caps();
    let watcher =
        HotReloadRegistry::new(vec![root.path().into()], TestRegistrar::new(), caps.clone());
    watcher.tick();
    let native = crate::local_recording::LocalRecordingRegistry::default();
    native.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
        },
        false,
    );
    assert_eq!(
        caps.local_recording.declaration("trigger"),
        native.declaration("trigger")
    );
    assert_eq!(
        caps.local_recording.set_enabled("trigger", true),
        native.set_enabled("trigger", true)
    );
    assert!(caps.tab_menu.contains("trigger"));
    assert!(!caps.webhook_receiver.contains("trigger"));

    write(&dir.join("grants.yaml"), "local_recording: true\n");
    set_mtime(
        &dir.join("grants.yaml"),
        SystemTime::UNIX_EPOCH + Duration::from_secs(100),
    );
    watcher.tick();
    assert!(caps.local_recording.status("trigger").unwrap().granted);
    let devices = SelectedDevices {
        input_id: "selected".into(),
        collection_id: "collection".into(),
        microphone_id: "mic".into(),
    };
    caps.local_recording
        .select_devices("trigger", devices.clone())
        .unwrap();
    caps.local_recording.set_enabled("trigger", true).unwrap();
    let setup = caps
        .local_recording
        .begin_subscription_setup("trigger")
        .unwrap();
    let generation = setup.generation;
    caps.local_recording
        .complete_subscription_setup(&setup, Box::new(ReadySubscription))
        .unwrap();
    let active = caps
        .local_recording
        .begin("trigger", &devices, generation)
        .unwrap();
    write(&dir.join("grants.yaml"), "local_recording: false\n");
    set_mtime(
        &dir.join("grants.yaml"),
        SystemTime::UNIX_EPOCH + Duration::from_secs(101),
    );
    watcher.tick();
    assert!(!active.load(Ordering::Acquire));
    assert!(!caps.local_recording.status("trigger").unwrap().granted);
    assert!(!caps.local_recording.mark_unavailable("trigger", generation));
    caps.deregister_plugin("trigger");
    assert!(!caps.local_recording.contains("trigger"));
}
