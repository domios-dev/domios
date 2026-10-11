use super::*;

fn declaration() -> LocalRecordingTrigger {
    LocalRecordingTrigger {
        vendor_id: 1,
        product_id: 2,
        usage_page: 3,
        usage: 4,
    }
}

fn devices(input_id: &str) -> SelectedDevices {
    SelectedDevices {
        input_id: input_id.into(),
        collection_id: "collection".into(),
        microphone_id: "mic".into(),
    }
}

fn configured(registry: &LocalRecordingRegistry, id: &str, selected: SelectedDevices) {
    registry.register(id, declaration(), true);
    registry.select_devices(id, selected).unwrap();
    registry.set_enabled(id, true).unwrap();
}

struct Subscription {
    stopped: Arc<AtomicBool>,
    teardown: Option<SubscriptionTeardown>,
}

impl LocalInputSubscription for Subscription {
    fn request_stop(&mut self) -> SubscriptionTeardown {
        self.stopped.store(true, Ordering::Release);
        self.teardown.take().unwrap()
    }
}

fn pending_subscription() -> (
    Box<dyn LocalInputSubscription>,
    Arc<AtomicBool>,
    std::sync::mpsc::Sender<Result<(), String>>,
) {
    let stopped = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    (
        Box::new(Subscription {
            stopped: stopped.clone(),
            teardown: Some(SubscriptionTeardown::new(rx)),
        }),
        stopped,
        tx,
    )
}

fn completed_subscription() -> (Box<dyn LocalInputSubscription>, Arc<AtomicBool>) {
    let (subscription, stopped, tx) = pending_subscription();
    tx.send(Ok(())).unwrap();
    (subscription, stopped)
}

fn ready(registry: &LocalRecordingRegistry, id: &str, selected: SelectedDevices) -> u64 {
    configured(registry, id, selected);
    let setup = registry.begin_subscription_setup(id).unwrap();
    let generation = setup.generation;
    registry
        .complete_subscription_setup(&setup, completed_subscription().0)
        .unwrap();
    generation
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
        registry.begin("trigger", &devices("input"), 0).unwrap_err(),
        "local_recording_grant_required"
    );
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    assert!(!registry.status("trigger").unwrap().available);
    registry
        .complete_subscription_setup(&setup, completed_subscription().0)
        .unwrap();
    let active = registry
        .begin("trigger", &devices("input"), setup.generation)
        .unwrap();
    assert_eq!(
        registry.begin_subscription_setup("trigger").unwrap_err(),
        "subscription_cleanup_pending"
    );
    assert_eq!(
        registry
            .begin("trigger", &devices("input"), setup.generation)
            .unwrap_err(),
        "parallel_capture"
    );
    registry.revoke("trigger");
    assert!(!active.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().enabled);
    assert!(!registry.mark_unavailable("trigger", setup.generation));
}

#[test]
fn lark_recording_contract_unload_during_setup_retires_stale_generation() {
    let registry = LocalRecordingRegistry::default();
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    registry.unregister("trigger");
    registry.register("trigger", declaration(), true);
    registry
        .select_devices("trigger", devices("input"))
        .unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let (subscription, stopped, teardown) = pending_subscription();
    assert_eq!(
        registry.complete_subscription_setup(&setup, subscription),
        Err("subscription_stale")
    );
    assert!(stopped.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().available);
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    assert!(registry.begin_subscription_setup("trigger").is_err());
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
    assert!(registry.begin_subscription_setup("trigger").is_ok());
}

#[test]
fn lark_recording_contract_disable_during_setup_keeps_late_listener_unavailable() {
    let registry = LocalRecordingRegistry::default();
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    registry.set_enabled("trigger", false).unwrap();
    let (subscription, stopped, teardown) = pending_subscription();
    assert_eq!(
        registry.complete_subscription_setup(&setup, subscription),
        Err("subscription_stale")
    );
    assert!(stopped.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().available);
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    teardown.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
}

#[test]
fn lark_recording_contract_pre_acquisition_setup_failure_releases_reservation() {
    let registry = LocalRecordingRegistry::default();
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    assert_eq!(
        registry.fail_subscription_setup(
            &setup,
            LocalInputSetupError {
                reason: "permission_denied".into(),
                teardown: None,
            }
        ),
        "permission_denied"
    );
    assert!(!registry.status("trigger").unwrap().available);
    assert!(registry.begin_subscription_setup("trigger").is_ok());
}

#[test]
fn lark_recording_contract_setup_failure_and_cleanup_timeout_keep_reservation() {
    let registry = LocalRecordingRegistry::default();
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reason = registry.fail_subscription_setup(
        &setup,
        LocalInputSetupError {
            reason: "native_setup_failed".into(),
            teardown: Some(SubscriptionTeardown::new(rx)),
        },
    );
    assert_eq!(reason, "native_setup_failed");
    assert!(!registry.status("trigger").unwrap().available);
    registry.unregister("trigger");
    registry.register("trigger", declaration(), true);
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    assert!(registry.begin_subscription_setup("trigger").is_err());
    tx.send(Err("native_wake_failed".into())).unwrap();
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    assert!(registry.begin_subscription_setup("trigger").is_err());
    tx.send(Ok(())).unwrap();
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Ok(())
    );
    configured(&registry, "trigger", devices("input"));
    assert!(registry.begin_subscription_setup("trigger").is_ok());
}

#[test]
fn lark_recording_contract_unload_re_register_waits_for_actual_cleanup() {
    let registry = LocalRecordingRegistry::default();
    configured(&registry, "trigger", devices("input"));
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    let (subscription, stopped, tx) = pending_subscription();
    registry
        .complete_subscription_setup(&setup, subscription)
        .unwrap();
    registry.unregister("trigger");
    assert!(stopped.load(Ordering::Acquire));
    registry.register("trigger", declaration(), true);
    registry
        .select_devices("trigger", devices("input"))
        .unwrap();
    registry.set_enabled("trigger", true).unwrap();
    assert_eq!(
        registry.wait_for_subscription_slot("trigger", Duration::ZERO),
        Err("subscription_cleanup_pending")
    );
    assert!(registry.begin_subscription_setup("trigger").is_err());
    tx.send(Ok(())).unwrap();
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
    let next = registry.begin_subscription_setup("trigger").unwrap();
    assert!(!registry.status("trigger").unwrap().available);
    registry
        .complete_subscription_setup(&next, completed_subscription().0)
        .unwrap();
    assert!(registry.status("trigger").unwrap().available);
    assert!(!registry.status("trigger").unwrap().capture_active);
}

#[test]
fn lark_recording_contract_removal_without_press_invalidates_capture() {
    let registry = LocalRecordingRegistry::default();
    let generation = ready(&registry, "trigger", devices("input"));
    let active = registry
        .begin("trigger", &devices("input"), generation)
        .unwrap();
    assert!(registry.mark_unavailable("trigger", generation));
    assert!(!active.load(Ordering::Acquire));
    assert!(!registry.status("trigger").unwrap().available);
    assert!(!registry.status("trigger").unwrap().capture_active);
    assert!(registry
        .begin("trigger", &devices("input"), generation)
        .is_err());
}

#[test]
fn lark_recording_contract_invalidation_observer_receives_old_generation_off_locks() {
    fn assert_reason(
        expected: LocalRecordingInvalidationReason,
        invalidate: impl FnOnce(&LocalRecordingRegistry, u64),
    ) {
        let registry = Arc::new(LocalRecordingRegistry::default());
        let generation = ready(&registry, "trigger", devices("input"));
        let active = registry
            .begin("trigger", &devices("input"), generation)
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let weak = Arc::downgrade(&registry);
        let observed_events = events.clone();
        let observed_active = active.clone();
        registry.set_invalidation_observer(Arc::new(move |event| {
            let registry = weak.upgrade().unwrap();
            assert!(registry.state.try_lock().is_some());
            assert!(registry.invalidation_observer.try_lock().is_some());
            let _ = registry.status("trigger");
            assert!(!observed_active.load(Ordering::Acquire));
            observed_events.lock().push(event);
        }));
        invalidate(&registry, generation);
        let events = events.lock();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].plugin_id, "trigger");
        assert_eq!(events[0].generation, generation);
        assert_eq!(events[0].reason, expected);
        assert!(!active.load(Ordering::Acquire));
    }

    assert_reason(LocalRecordingInvalidationReason::Disabled, |registry, _| {
        registry.set_enabled("trigger", false).unwrap();
    });
    assert_reason(
        LocalRecordingInvalidationReason::GrantRevoked,
        |registry, _| {
            registry.revoke("trigger");
        },
    );
    assert_reason(
        LocalRecordingInvalidationReason::Unregistered,
        |registry, _| {
            registry.unregister("trigger");
        },
    );
    assert_reason(
        LocalRecordingInvalidationReason::DeviceSelectionChanged,
        |registry, _| {
            registry
                .select_devices("trigger", devices("replacement"))
                .unwrap();
        },
    );
    assert_reason(
        LocalRecordingInvalidationReason::DeviceRemoved,
        |registry, generation| {
            assert!(registry.mark_unavailable_with_reason(
                "trigger",
                generation,
                LocalRecordingInvalidationReason::DeviceRemoved,
            ));
        },
    );
    assert_reason(
        LocalRecordingInvalidationReason::AccessLost,
        |registry, generation| {
            assert!(registry.mark_unavailable_with_reason(
                "trigger",
                generation,
                LocalRecordingInvalidationReason::AccessLost,
            ));
        },
    );
    assert_reason(
        LocalRecordingInvalidationReason::DeviceUnavailable,
        |registry, generation| {
            assert!(registry.mark_unavailable("trigger", generation));
        },
    );
}

#[test]
fn lark_recording_contract_stale_unavailable_generation_keeps_new_capture() {
    let registry = Arc::new(LocalRecordingRegistry::default());
    let old_generation = ready(&registry, "trigger", devices("input"));
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let observed = notifications.clone();
    registry.set_invalidation_observer(Arc::new(move |event| observed.lock().push(event)));
    let old_capture = registry
        .begin("trigger", &devices("input"), old_generation)
        .unwrap();
    let next_generation = registry
        .select_devices("trigger", devices("replacement"))
        .unwrap();
    assert_ne!(next_generation, old_generation);
    assert!(!old_capture.load(Ordering::Acquire));
    registry
        .wait_for_subscription_slot("trigger", Duration::ZERO)
        .unwrap();
    let setup = registry.begin_subscription_setup("trigger").unwrap();
    assert_eq!(setup.generation, next_generation);
    let (selected_devices, selected_generation) = registry.selected_devices("trigger").unwrap();
    assert_eq!(selected_devices, devices("replacement"));
    assert_eq!(selected_generation, setup.generation);
    registry
        .complete_subscription_setup(&setup, completed_subscription().0)
        .unwrap();
    let current_generation = registry.status("trigger").unwrap().generation;
    assert_ne!(current_generation, old_generation);
    assert_eq!(current_generation, next_generation);
    let current_capture = registry
        .begin("trigger", &devices("replacement"), current_generation)
        .unwrap();
    assert!(!registry.mark_unavailable_with_reason(
        "trigger",
        old_generation,
        LocalRecordingInvalidationReason::DeviceRemoved,
    ));
    assert!(current_capture.load(Ordering::Acquire));
    assert_eq!(notifications.lock().len(), 1);
    assert_eq!(notifications.lock()[0].generation, old_generation);
}
