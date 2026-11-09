use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn cancellation_completes_an_overlapped_write_with_a_held_reader() {
    let (_held_reader, mut writer) = native_pipe().unwrap();
    let cancelled = writer.cancelled.clone();
    let (entered, observed) = mpsc::sync_channel(1);
    let (finished, result) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result =
            writer.write_with_pending_hook(&vec![7; 1024 * 1024], || entered.send(()).unwrap());
        finished.send(result).unwrap();
    });
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_ne!(unsafe { SetEvent(cancelled.as_raw_handle() as _) }, 0);
    assert!(result
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_err());
    worker.join().unwrap();
}

#[test]
fn cancellation_before_write_does_not_submit_bytes() {
    let (_held_reader, mut writer) = native_pipe().unwrap();
    assert_ne!(
        unsafe { SetEvent(writer.cancelled.as_raw_handle() as _) },
        0
    );
    let error = writer
        .write_with_pending_hook(b"never submitted", || panic!("cancelled write submitted"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}
