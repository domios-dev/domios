use super::*;
use std::cell::Cell;

fn identity() -> SelectedInputIdentity {
    SelectedInputIdentity {
        input_id:
            r"\\?\HID#VID_3547&PID_0008&MI_00&Col01#fixture#{4d1e55b2-f16f-11cf-88cb-001111000030}"
                .into(),
        collection_id: COLLECTION_ID.into(),
    }
}

fn raw_button_capability(usage: u16, data_index: u16) -> ButtonCapability {
    ButtonCapability {
        usage_page: CONSUMER_CONTROL_PAGE,
        report_id: BUTTON_REPORT_ID,
        usage,
        data_index,
    }
}

fn supported_test_decoder(generation: u64) -> SelectedReportDecoder {
    SelectedReportDecoder::new(identity(), HidDescriptor::supported(), generation).unwrap()
}

fn decode(
    decoder: &mut SelectedReportDecoder,
    generation: u64,
    actual_identity: &SelectedInputIdentity,
    report: &[u8],
    usages: &[u16],
    now_ms: u64,
) -> DecodeResult {
    decoder.decode(
        generation,
        actual_identity,
        report,
        CONSUMER_CONTROL_PAGE,
        usages,
        now_ms,
    )
}

#[test]
fn lark_input_windows_descriptor_caps_match_only_the_observed_schema() {
    let descriptor = HidDescriptor::supported();
    assert!(descriptor.matches_supported_schema());

    let mut unknown = descriptor.clone();
    unknown.input_report_bytes += 1;
    assert!(!unknown.matches_supported_schema());

    let mut wrong_usage = descriptor;
    wrong_usage.button_capabilities[7] = raw_button_capability(0x00EA, 7);
    assert!(!wrong_usage.matches_supported_schema());
}

#[test]
fn lark_input_windows_report_pair_emits_one_press_and_no_release_toggle() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Press
    );
    assert_eq!(
        decode(&mut decoder, 7, &identity(), &[0x06, 0x00, 0x00], &[], 110),
        DecodeResult::Ignored
    );
}

#[test]
fn lark_input_windows_duplicate_pressed_state_is_suppressed() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Press
    );
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            101
        ),
        DecodeResult::Ignored
    );
}

#[test]
fn lark_input_windows_rapid_release_reassertion_is_duplicate_suppressed() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Press
    );
    assert_eq!(
        decode(&mut decoder, 7, &identity(), &[0x06, 0x00, 0x00], &[], 110),
        DecodeResult::Ignored
    );
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            120
        ),
        DecodeResult::Ignored
    );
    assert_eq!(
        decode(&mut decoder, 7, &identity(), &[0x06, 0x00, 0x00], &[], 130),
        DecodeResult::Ignored
    );
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            150
        ),
        DecodeResult::Press
    );
}

#[test]
fn lark_input_windows_release_without_prior_press_is_inert() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(&mut decoder, 7, &identity(), &[0x06, 0x00, 0x00], &[], 100),
        DecodeResult::Ignored
    );
}

#[test]
fn lark_input_windows_foreign_device_and_collection_are_inert() {
    let mut decoder = supported_test_decoder(7);
    let mut foreign_device = identity();
    foreign_device.input_id = foreign_device.input_id.replace("Col01", "Col02");
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &foreign_device,
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Ignored
    );

    let mut foreign_collection = identity();
    foreign_collection.collection_id = "Col02".into();
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &foreign_collection,
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Ignored
    );
}

#[test]
fn lark_input_windows_malformed_report_and_foreign_usage_are_inert() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x05, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Malformed
    );
    assert_eq!(
        decode(&mut decoder, 7, &identity(), &[0x06, 0x01], &[0x00E9], 100),
        DecodeResult::Malformed
    );
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x0001],
            100
        ),
        DecodeResult::Malformed
    );
}

#[test]
fn lark_input_windows_report_count_and_length_are_bounded() {
    let payload = [0x06, 0x01, 0x00, 0x06, 0x00, 0x00];
    assert_eq!(
        split_report_batch(2, 3, &payload, BUTTON_REPORT_BYTES).unwrap(),
        vec![&payload[..3], &payload[3..]]
    );
    assert!(split_report_batch(0, 3, &[], BUTTON_REPORT_BYTES).is_none());
    assert!(split_report_batch(9, 3, &[0; 27], BUTTON_REPORT_BYTES).is_none());
    assert!(split_report_batch(1, 65, &[0; 65], BUTTON_REPORT_BYTES).is_none());
    assert!(split_report_batch(2, 3, &[0; 5], BUTTON_REPORT_BYTES).is_none());
}

#[test]
fn lark_input_windows_stale_generation_is_inert() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decode(
            &mut decoder,
            6,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Ignored
    );
}

#[test]
fn lark_input_windows_removal_requires_explicit_rearm_without_synthetic_press() {
    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decoder.invalidate(UnavailableReason::Removed),
        Some(WindowsInputEvent::Unavailable {
            generation: 7,
            reason: UnavailableReason::Removed,
        })
    );
    assert_eq!(decoder.invalidate(UnavailableReason::Removed), None);
    assert_eq!(
        decode(
            &mut decoder,
            7,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            100
        ),
        DecodeResult::Ignored
    );
    let mut rearmed = supported_test_decoder(8);
    assert_eq!(rearmed.generation, 8);
    assert!(!rearmed.pressed);
    assert_eq!(
        decode(
            &mut rearmed,
            8,
            &identity(),
            &[0x06, 0x01, 0x00],
            &[0x00E9],
            200
        ),
        DecodeResult::Press
    );
}

#[test]
fn lark_input_windows_access_loss_and_other_errors_are_distinct() {
    assert_eq!(
        unavailable_reason_for_win32(ERROR_ACCESS_DENIED),
        UnavailableReason::AccessLost
    );
    assert_eq!(
        unavailable_reason_for_win32(ERROR_DEVICE_NOT_CONNECTED),
        UnavailableReason::Removed
    );
    assert_eq!(unavailable_reason_for_win32(87), UnavailableReason::Error);

    let mut decoder = supported_test_decoder(7);
    assert_eq!(
        decoder.invalidate(UnavailableReason::AccessLost),
        Some(WindowsInputEvent::Unavailable {
            generation: 7,
            reason: UnavailableReason::AccessLost,
        })
    );
}

#[test]
fn lark_input_windows_teardown_ack_follows_resource_release() {
    let released = Cell::new(false);
    let (sender, receiver) = mpsc::channel();
    complete_teardown(
        || {
            released.set(true);
            Ok(())
        },
        sender,
    );

    assert_eq!(receiver.recv().unwrap(), Ok(()));
    assert!(released.get());

    let (sender, receiver) = mpsc::channel();
    complete_teardown(|| Err(TeardownError::WindowDestroy(5)), sender);
    assert_eq!(
        receiver.recv().unwrap(),
        Err(TeardownError::WindowDestroy(5))
    );
}
