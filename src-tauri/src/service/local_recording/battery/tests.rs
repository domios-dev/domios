use super::*;
fn frame() -> [u8; 64] {
    let mut frame = [0; 64];
    frame[..16].copy_from_slice(&[
        0x55, 0xbb, 0xdd, 0x10, 0x80, 0, 9, 0, 1, 0, 97, 0, 0, 0, 4, 0,
    ]);
    frame
}
#[test]
fn lark_battery_observed_reply_distinguishes_off_and_97_percent() {
    let batteries = decode_status(&frame()).unwrap();
    assert_eq!(
        batteries[0],
        TransmitterBattery {
            slot: 1,
            connected: false,
            percent: None
        }
    );
    assert_eq!(
        batteries[1],
        TransmitterBattery {
            slot: 2,
            connected: true,
            percent: Some(97)
        }
    );
}
#[test]
fn lark_battery_rejects_foreign_and_malformed_frames() {
    assert!(decode_status(&frame()[..63]).is_err());
    for (index, value) in [
        (0, 0),
        (3, 0x11),
        (4, 0x40),
        (5, 1),
        (6, 3),
        (7, 2),
        (9, 101),
        (10, 255),
    ] {
        let mut report = frame();
        report[index] = value;
        assert!(decode_status(&report).is_err());
    }
}
#[test]
fn lark_battery_zero_is_valid_for_a_connected_transmitter() {
    let mut report = frame();
    report[10] = 0;
    assert_eq!(decode_status(&report).unwrap()[1].percent, Some(0));
}
