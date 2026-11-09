use super::*;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn cancellation_wakes_a_full_descriptor_with_a_held_reader() {
    let (output, _held_reader) = UnixStream::pair().unwrap();
    let output = unsafe { FileDescriptor::from_raw_fd(output.into_raw_fd()) };
    let (mut writer, cancelled) = native_writer(output).unwrap();
    let (entered, observed) = mpsc::sync_channel(1);
    let (finished, result) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let bytes = vec![7; 1024 * 1024];
        let mut offset = 0;
        let result = loop {
            match writer.write_with_wait_hook(&bytes[offset..], || entered.send(()).unwrap()) {
                Ok(written) => {
                    offset = (offset + written) % bytes.len();
                }
                Err(error) => break error,
            }
        };
        finished.send(result).unwrap();
    });
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    cancelled.shutdown(Shutdown::Write).unwrap();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(2)).unwrap().kind(),
        io::ErrorKind::BrokenPipe
    );
    worker.join().unwrap();
}

#[test]
fn cancellation_before_write_preserves_empty_input() {
    let (output, mut input) = UnixStream::pair().unwrap();
    input.set_nonblocking(true).unwrap();
    let output = unsafe { FileDescriptor::from_raw_fd(output.into_raw_fd()) };
    let (mut writer, cancelled) = native_writer(output).unwrap();
    cancelled.shutdown(Shutdown::Write).unwrap();
    assert_eq!(
        writer.write(b"never submitted").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    let mut byte = [0];
    assert_eq!(
        std::io::Read::read(&mut input, &mut byte)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WouldBlock
    );
}
