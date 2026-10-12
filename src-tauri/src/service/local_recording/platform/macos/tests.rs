use super::*;
use std::sync::{Arc, Mutex};

struct FakeRunLoopResources(Arc<Mutex<Vec<&'static str>>>);

impl RunLoopResourceOps for FakeRunLoopResources {
    fn stop_and_wake(&mut self) {
        let mut events = self.0.lock().unwrap();
        events.push("stop");
        events.push("wake");
    }

    fn release(&mut self) {
        self.0.lock().unwrap().push("release");
    }
}

fn fake_run_loop(events: Arc<Mutex<Vec<&'static str>>>) -> RunLoopHandle {
    RunLoopHandle::new(FakeRunLoopResources(events))
}

fn run_loop_events(events: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
    events.lock().unwrap().clone()
}

fn device_id(location_id: u32, registry_entry_id: u64) -> String {
    format!("{DEVICE_ID_PREFIX}{location_id:08x}:{registry_entry_id:016x}")
}

fn collection_id(cookie: u32, usage_page: u16, usage: u16) -> String {
    format!("{COLLECTION_ID_PREFIX}{cookie:08x}:{usage_page:04x}:{usage:04x}")
}

fn selection() -> MacSelection {
    MacSelection {
        input_id: device_id(0x1234, 0x123456789abcdef0),
        collection_id: collection_id(7, 0x000c, 1),
        vendor_id: 0x1234,
        product_id: 0x5678,
        collection_usage_page: 0x000c,
        collection_usage: 0x0001,
        approved_element_usage: Some(MacElementUsage {
            usage_page: 0xff00,
            usage: 0x20,
        }),
    }
}

fn descriptor() -> ElementDescriptor {
    ElementDescriptor {
        cookie: 19,
        usage_page: 0xff00,
        usage: 0x20,
        kind: ElementKind::Button,
        logical_min: 0,
        logical_max: 1,
        report_size: 1,
        report_count: 1,
        is_array: false,
        is_relative: false,
        application_collections: vec![CollectionIdentity {
            cookie: 7,
            usage_page: 0x000c,
            usage: 1,
        }],
    }
}

fn decoder() -> InputDecoder {
    let selected = selection();
    let approved = approve_descriptor(&selected, &selected.input_id, &[descriptor()]).unwrap();
    InputDecoder::new(&selected, approved, 41)
}

#[test]
fn lark_input_macos_descriptor_approved_press_and_release() {
    let selected = selection();
    let mut decoder = decoder();
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 1),
        Some(MacInputEvent::Press { generation: 41 })
    );
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 0),
        None
    );
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 1),
        Some(MacInputEvent::Press { generation: 41 })
    );
}

#[test]
fn lark_input_macos_duplicate_pressed_state_is_suppressed() {
    let selected = selection();
    let mut decoder = decoder();
    assert!(decoder
        .observe(&selected.input_id, &selected.collection_id, 19, 1)
        .is_some());
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 1),
        None
    );
}

#[test]
fn lark_input_macos_wrong_device_is_inert() {
    let selected = selection();
    let mut decoder = decoder();
    let other = device_id(0x1235, 0x123456789abcdef1);
    assert_eq!(
        decoder.observe(&other, &selected.collection_id, 19, 1),
        None
    );
    assert!(decoder
        .observe(&selected.input_id, &selected.collection_id, 19, 1)
        .is_some());
}

#[test]
fn lark_input_macos_wrong_collection_is_inert() {
    let selected = selection();
    let mut decoder = decoder();
    let other = collection_id(8, 0xff00, 1);
    assert_eq!(decoder.observe(&selected.input_id, &other, 19, 1), None);
    assert!(decoder
        .observe(&selected.input_id, &selected.collection_id, 19, 1)
        .is_some());
}

#[test]
fn lark_input_macos_wrong_element_is_inert() {
    let selected = selection();
    let mut decoder = decoder();
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 20, 1),
        None
    );
    assert!(decoder
        .observe(&selected.input_id, &selected.collection_id, 19, 1)
        .is_some());
}

#[test]
fn lark_input_macos_malformed_value_is_inert() {
    let selected = selection();
    let mut decoder = decoder();
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 2),
        None
    );
    assert!(decoder
        .observe(&selected.input_id, &selected.collection_id, 19, 1)
        .is_some());
}

#[test]
fn lark_input_macos_nonbinary_or_ambiguous_descriptor_fails_closed() {
    let selected = selection();
    let mut invalid = descriptor();
    invalid.logical_max = 2;
    assert_eq!(
        approve_descriptor(&selected, &selected.input_id, &[invalid]),
        Err(DescriptorError::InvalidElement)
    );
    assert_eq!(
        approve_descriptor(&selected, &selected.input_id, &[descriptor(), descriptor()]),
        Err(DescriptorError::AmbiguousElement)
    );
}

#[test]
fn lark_input_macos_missing_approved_element_mapping_fails_setup_closed() {
    let mut selected = selection();
    selected.approved_element_usage = None;
    assert_eq!(
        setup_validation_error(&selected),
        Some("macos_hid_element_mapping_unavailable")
    );
    assert_eq!(
        approve_descriptor(&selected, &selected.input_id, &[descriptor()]),
        Err(DescriptorError::MissingApprovedMapping)
    );
}

#[test]
fn lark_input_macos_collection_declaration_is_separate_from_element_mapping() {
    let selected = selection();
    assert_eq!(
        approve_descriptor(&selected, &selected.input_id, &[descriptor()]),
        Ok(ApprovedElement { cookie: 19 })
    );
    let mut mismatched = selected.clone();
    mismatched.collection_usage = 2;
    assert_eq!(
        approve_descriptor(&mismatched, &mismatched.input_id, &[descriptor()]),
        Err(DescriptorError::WrongCollection)
    );
}

#[test]
fn lark_input_macos_teardown_wait_requires_reservation_until_release() {
    let (_sender, receiver) = std::sync::mpsc::sync_channel(1);
    let pending = classify_teardown_wait(receiver.recv_timeout(std::time::Duration::ZERO));
    assert_eq!(pending, TeardownWaitOutcome::Pending);
    assert!(teardown_reservation_required(&pending));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    drop(sender);
    let disconnected = classify_teardown_wait(receiver.recv_timeout(std::time::Duration::ZERO));
    assert!(teardown_reservation_required(&disconnected));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender.send(Err("close failed".into())).unwrap();
    let failed = classify_teardown_wait(receiver.recv_timeout(std::time::Duration::ZERO));
    assert!(teardown_reservation_required(&failed));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender.send(Ok(())).unwrap();
    let released = classify_teardown_wait(receiver.recv_timeout(std::time::Duration::ZERO));
    assert!(!teardown_reservation_required(&released));
}

#[test]
fn lark_input_macos_wrong_identity_and_collection_fail_closed() {
    let selected = selection();
    assert_eq!(
        approve_descriptor(
            &selected,
            &device_id(0x1235, 0x123456789abcdef1),
            &[descriptor()]
        ),
        Err(DescriptorError::WrongDevice)
    );
    let mut element = descriptor();
    element.application_collections.clear();
    assert_eq!(
        approve_descriptor(&selected, &selected.input_id, &[element]),
        Err(DescriptorError::MissingElement)
    );
}

#[test]
fn lark_input_macos_denied_access_is_distinct_from_native_error() {
    assert_eq!(
        access_failure_reason(AccessCheck::Denied),
        MacUnavailableReason::AccessLost
    );
    assert_eq!(
        access_failure_reason(AccessCheck::Unknown),
        MacUnavailableReason::AccessLost
    );
    assert_eq!(
        access_failure_reason(AccessCheck::Granted),
        MacUnavailableReason::Error
    );
}

#[test]
fn lark_input_macos_access_loss_invalidates_generation() {
    let selected = selection();
    let mut decoder = decoder();
    assert_eq!(
        decoder.unavailable(MacUnavailableReason::AccessLost),
        Some(MacInputEvent::Unavailable {
            generation: 41,
            reason: MacUnavailableReason::AccessLost,
        })
    );
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 1),
        None
    );
}

#[test]
fn lark_input_macos_removal_and_unload_invalidate_callbacks() {
    let selected = selection();
    let mut removed = decoder();
    assert_eq!(
        removed.unavailable(MacUnavailableReason::Removed),
        Some(MacInputEvent::Unavailable {
            generation: 41,
            reason: MacUnavailableReason::Removed,
        })
    );
    assert_eq!(
        removed.observe(&selected.input_id, &selected.collection_id, 19, 1),
        None
    );

    let mut unloaded = decoder();
    unloaded.active = false;
    assert_eq!(
        unloaded.observe(&selected.input_id, &selected.collection_id, 19, 1),
        None
    );
    assert_eq!(unloaded.unavailable(MacUnavailableReason::Removed), None);
}

#[test]
fn lark_input_macos_stale_callback_after_stop_is_inert() {
    let selected = selection();
    let mut decoder = decoder();
    decoder.active = false;
    assert_eq!(
        decoder.observe(&selected.input_id, &selected.collection_id, 19, 1),
        None
    );
}

#[test]
fn lark_input_macos_queued_readiness_abandonment_stops_and_releases_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);

    send_ready_run_loop(ready_tx, fake_run_loop(events.clone()));
    drop(ready_rx);

    assert_eq!(run_loop_events(&events), ["stop", "wake", "release"]);
    assert!(matches!(
        done_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    done_tx.send(Ok(())).unwrap();
    assert_eq!(done_rx.try_recv(), Ok(Ok(())));
    assert_eq!(run_loop_events(&events), ["stop", "wake", "release"]);
}

#[test]
fn lark_input_macos_readiness_transfer_keeps_owner_until_cleanup() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

    send_ready_run_loop(ready_tx, fake_run_loop(events.clone()));
    let mut owner = ready_rx.recv().unwrap().unwrap();
    assert!(run_loop_events(&events).is_empty());

    owner.stop();
    owner.stop();
    drop(owner);
    assert_eq!(run_loop_events(&events), ["stop", "wake", "release"]);
}

#[test]
fn lark_input_macos_failed_readiness_send_retires_owner_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    drop(ready_rx);

    send_ready_run_loop(ready_tx, fake_run_loop(events.clone()));

    assert_eq!(run_loop_events(&events), ["stop", "wake", "release"]);
}

#[test]
fn lark_input_macos_missing_loop_entry_retires_untransferred_owner_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

    abandon_ready_run_loop(
        ready_tx,
        "macos_input_start_unacknowledged",
        Some(fake_run_loop(events.clone())),
    );

    assert!(matches!(
        ready_rx.try_recv(),
        Ok(Err(reason)) if reason == "macos_input_start_unacknowledged"
    ));
    assert_eq!(run_loop_events(&events), ["stop", "wake", "release"]);
}
