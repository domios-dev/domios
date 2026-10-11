use super::tests::scene;
use super::*;
use codeterm_plugin::local_recording::LocalRecordingTrigger;

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn lark_recording_contract_local_recipient_without_webhook_and_closed_choice_tombstone() {
    let scene = scene();
    let service = &scene.fixture.service;
    service.local_recording_registry.register(
        "local",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
        },
        true,
    );
    assert!(!service.webhook_registry.contains("local"));
    service.set_receiver_target("local", &scene.tab).unwrap();
    service.set_receiver_target("receiver", &scene.tab).unwrap();
    service.close_tab(&scene.tab).unwrap();
    assert_eq!(service.receiver_targets.get("receiver"), None);
    assert_eq!(
        service.local_recording_targets.get("local"),
        Some(scene.tab.clone())
    );
    service.local_recording_registry.unregister("local");
    service.forget_receiver_targets_for_tab(&scene.tab);
    assert_eq!(
        service.local_recording_targets.get("local"),
        Some(scene.tab.clone())
    );
    service.local_recording_registry.register(
        "local",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
        },
        true,
    );
    assert!(!service.receiver_target("local").unwrap().live);
    let selected = codeterm_plugin::local_recording::SelectedDevices {
        input_id: "selected".into(),
        collection_id: "selected-collection".into(),
        microphone_id: "selected-mic".into(),
    };
    assert!(service
        .local_recording_start("local", &selected, 0)
        .is_err());
    assert_eq!(
        service.receiver_target("local").unwrap().tab_id,
        Some(scene.tab.clone())
    );
    service.clear_receiver_target("local").unwrap();
    assert_eq!(service.receiver_target("local").unwrap().tab_id, None);
}
