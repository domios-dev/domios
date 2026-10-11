use super::*;
use codeterm_plugin::local_recording::LocalRecordingTrigger;

pub(in crate::service::local_recording) fn target() -> FrozenTarget {
    FrozenTarget::Chat(ChatIdentity {
        tab_id: "A".into(),
        plugin_id: "backend".into(),
        session_id: "S".into(),
        binding_nonce: "N".into(),
    })
}

pub(in crate::service::local_recording) fn armed() -> (LocalRecordingRegistry, SelectedDevices, u64)
{
    let registry = LocalRecordingRegistry::default();
    registry.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
        },
        true,
    );
    let devices = SelectedDevices {
        input_id: "selected-input".into(),
        collection_id: "selected-collection".into(),
        microphone_id: "selected-mic".into(),
    };
    let generation = registry.select_devices("trigger", devices.clone()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = registry
        .selected_devices("trigger")
        .unwrap()
        .1
        .max(generation);
    assert!(registry.set_available("trigger", generation, true));
    (registry, devices, generation)
}

#[test]
fn lark_recording_contract_general_resolves_at_start_and_explicit_close_never_falls_back() {
    assert_eq!(
        resolve_target(None, || Some("current-General".into())).unwrap(),
        "current-General"
    );
    assert_eq!(
        resolve_target(Some("closed-A".into()), || panic!(
            "explicit choice fell back"
        ))
        .unwrap(),
        "closed-A"
    );
    assert!(resolve_target(None, || None).is_err());
}

#[test]
fn lark_recording_contract_selected_device_manual_parallel_and_actual_stop() {
    let (registry, devices, generation) = armed();
    let leases = RecordingLeases::default();
    leases.set_manual_capture(true).unwrap();
    assert!(leases
        .start("trigger", &devices, generation, target(), &registry)
        .is_err());
    leases.set_manual_capture(false).unwrap();
    let lease = leases
        .start("trigger", &devices, generation, target(), &registry)
        .unwrap();
    assert_eq!(lease.microphone_id, "selected-mic");
    assert!(leases.set_manual_capture(true).is_err());
    assert!(leases
        .start("trigger", &devices, generation, target(), &registry)
        .is_err());
    assert!(leases.transcribing(&lease.lease).is_err());
    leases.stopped(&lease.lease).unwrap();
    leases.transcribing(&lease.lease).unwrap();
    assert!(leases.transcribing(&lease.lease).is_err());
}

#[test]
fn lark_recording_contract_close_during_capture_revoke_unload_and_expiry() {
    for invalidate in [0, 1, 2, 3] {
        let (registry, devices, generation) = armed();
        let leases = RecordingLeases::default();
        let lease = leases
            .start("trigger", &devices, generation, target(), &registry)
            .unwrap();
        leases.stopped(&lease.lease).unwrap();
        let capture = leases.transcribing(&lease.lease).unwrap();
        match invalidate {
            0 => leases.close_tab("A"),
            1 => registry.revoke("trigger"),
            2 => registry.unregister("trigger"),
            _ => leases.cancel(&lease.lease),
        }
        assert!(capture.validate().is_err());
    }
    let capture = Capture {
        lease_id: "expired".into(),
        plugin_id: "trigger".into(),
        target: target(),
        active: Arc::new(AtomicBool::new(true)),
        expires_at: Arc::new(parking_lot::Mutex::new(Instant::now())),
        phase: CapturePhase::Stopped,
    };
    assert!(!capture.is_current(Instant::now()));
}

#[test]
fn lark_recording_contract_wrong_subscription_and_device_are_refused() {
    let (registry, devices, generation) = armed();
    let leases = RecordingLeases::default();
    let mut wrong = devices.clone();
    wrong.collection_id = "foreign-collection".into();
    assert!(leases
        .start("trigger", &wrong, generation, target(), &registry)
        .is_err());
    assert!(leases
        .start("trigger", &devices, generation + 1, target(), &registry)
        .is_err());
    registry.set_available("trigger", generation, false);
    assert!(leases
        .start("trigger", &devices, generation, target(), &registry)
        .is_err());
}
