use super::*;

fn declaration() -> LocalRecordingTrigger {
    LocalRecordingTrigger {
        vendor_id: 1,
        product_id: 2,
        usage_page: 3,
        usage: 4,
    }
}

fn devices() -> SelectedDevices {
    SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "mic".into(),
    }
}

#[test]
fn lark_recording_contract_manifest_menu_without_webhook_and_strict_schema() {
    use crate::js_runtime::PluginManifest;
    let manifest = serde_json::json!({
        "id": "trigger", "displayName": "Trigger", "version": "0.1.0",
        "capabilities": { "localRecordingTrigger": { "vendorId": 1, "productId": 2, "usagePage": 3, "usage": 4 }, "tabMenu": [{ "id": "receive", "label": "Receive here", "action": "receiverTarget" }] }
    });
    let parsed = PluginManifest::from_json_str(&manifest.to_string()).unwrap();
    assert!(parsed.capabilities.webhook_receiver.is_none());
    assert_eq!(
        parsed.capabilities.local_recording_trigger,
        Some(declaration())
    );
    let mut bad = manifest.clone();
    bad["capabilities"]["localRecordingTrigger"]["arbitraryDevice"] = serde_json::json!(true);
    assert!(PluginManifest::from_json_str(&bad.to_string()).is_err());
    bad = manifest;
    bad["capabilities"]["localRecordingTrigger"] = serde_json::Value::Null;
    assert!(PluginManifest::from_json_str(&bad.to_string()).is_err());
}

#[test]
fn lark_recording_contract_grant_denial_revocation_and_parallel_leases() {
    let registry = LocalRecordingRegistry::default();
    registry.register("trigger", declaration(), false);
    assert_eq!(
        registry.set_enabled("trigger", true),
        Err("local_recording_grant_required")
    );
    assert_eq!(
        registry.begin("trigger", &devices(), 0).unwrap_err(),
        "local_recording_grant_required"
    );
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = registry.selected_devices("trigger").unwrap().1;
    registry.set_available("trigger", generation, true);
    let active = registry.begin("trigger", &devices(), generation).unwrap();
    assert_eq!(
        registry
            .begin("trigger", &devices(), generation)
            .unwrap_err(),
        "parallel_capture"
    );
    registry.revoke("trigger");
    assert!(!active.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().enabled);
    assert!(!registry.set_available("trigger", generation, true));
}

struct Subscription(Arc<AtomicBool>);
impl LocalInputSubscription for Subscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        self.0.store(true, Ordering::Release);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok(())).unwrap();
        SubscriptionTeardown::new(rx)
    }
}

#[test]
fn lark_recording_contract_unload_stops_subscription_and_invalidates_callback() {
    let registry = LocalRecordingRegistry::default();
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = registry.selected_devices("trigger").unwrap().1;
    let stopped = Arc::new(AtomicBool::new(false));
    registry
        .attach_subscription(
            "trigger",
            generation,
            Box::new(Subscription(stopped.clone())),
        )
        .unwrap();
    registry.set_available("trigger", generation, true);
    let active = registry.begin("trigger", &devices(), generation).unwrap();
    registry.unregister("trigger");
    assert!(stopped.load(Ordering::Acquire));
    assert!(!active.load(Ordering::Acquire));
    registry.register("trigger", declaration(), true);
    assert!(registry.begin("trigger", &devices(), generation).is_err());
}

#[test]
fn lark_recording_contract_stop_request_precedes_teardown_acknowledgement() {
    let (tx, rx) = std::sync::mpsc::channel();
    let completion = SubscriptionTeardown::new(rx);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        entered_tx.send(()).unwrap();
        done_tx.send(completion.wait()).unwrap();
    });
    entered_rx.recv().unwrap();
    assert!(matches!(
        done_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    tx.send(Ok(())).unwrap();
    assert_eq!(done_rx.recv().unwrap(), Ok(()));
    worker.join().unwrap();
}

#[test]
fn lark_recording_contract_removal_without_press_invalidates_capture() {
    let registry = LocalRecordingRegistry::default();
    registry.register("trigger", declaration(), true);
    registry.select_devices("trigger", devices()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = registry.selected_devices("trigger").unwrap().1;
    assert!(registry.set_available("trigger", generation, true));
    let active = registry.begin("trigger", &devices(), generation).unwrap();
    assert!(registry.set_available("trigger", generation, false));
    assert!(!active.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().available);
    assert!(!registry.set_available("trigger", generation, true));
    assert!(registry.begin("trigger", &devices(), generation).is_err());
}
