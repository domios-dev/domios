use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Wake;

#[derive(Default)]
struct WakeCounter(AtomicUsize);

impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn poll(read: &mut BackendRead<'_>, waker: &Waker) -> Poll<Result<Vec<u8>, String>> {
    Pin::new(read).poll(&mut Context::from_waker(waker))
}

fn output() -> (BackendOutput, tokio::sync::mpsc::Sender<Vec<u8>>) {
    let output = BackendOutput::default();
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    output.install(receiver);
    (output, sender)
}

#[test]
fn remount_supersedes_parked_reader_without_consuming_its_output() {
    let (output, sender) = output();
    let wakes = Arc::new(WakeCounter::default());
    let waker = Waker::from(wakes.clone());
    let mut old = output.read();
    assert!(poll(&mut old, &waker).is_pending());

    sender.try_send(b"first".to_vec()).unwrap();
    let previous_wakes = wakes.0.load(Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut current = output.read();
    assert!(wakes.0.load(Ordering::Relaxed) > previous_wakes);
    assert_eq!(
        poll(&mut old, &waker),
        Poll::Ready(Err("PTY read superseded".to_string()))
    );
    assert_eq!(
        poll(&mut current, &waker),
        Poll::Ready(Ok(b"first".to_vec()))
    );
    eprintln!(
        "backend reader handoff: {} us",
        started.elapsed().as_micros()
    );

    drop(old);
    drop(current);
    let mut next = output.read();
    assert!(poll(&mut next, &waker).is_pending());
    sender.try_send(b"second".to_vec()).unwrap();
    assert_eq!(poll(&mut next, &waker), Poll::Ready(Ok(b"second".to_vec())));
}

#[test]
fn cancelled_read_preserves_receiver_and_queued_bytes() {
    let (output, sender) = output();
    let waker = Waker::noop();
    let mut cancelled = output.read();
    assert!(poll(&mut cancelled, waker).is_pending());
    drop(cancelled);
    sender.try_send(vec![0, 1, 255]).unwrap();
    let mut current = output.read();
    assert_eq!(poll(&mut current, waker), Poll::Ready(Ok(vec![0, 1, 255])));
}

#[test]
fn dropping_superseded_request_keeps_the_current_waiter() {
    let (output, sender) = output();
    let mut old = output.read();
    assert!(poll(&mut old, Waker::noop()).is_pending());
    let wakes = Arc::new(WakeCounter::default());
    let waker = Waker::from(wakes.clone());
    let mut current = output.read();
    assert!(poll(&mut current, &waker).is_pending());
    drop(old);
    output.close();
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert_eq!(
        poll(&mut current, &waker),
        Poll::Ready(Err("PTY closed".to_string()))
    );
    assert!(sender.try_send(vec![9]).is_err());
}

#[test]
fn rapid_remounts_preserve_order_and_coalesce_each_chunk_once() {
    let (output, sender) = output();
    let waker = Waker::noop();
    let mut readers = Vec::new();
    for _ in 0..100 {
        let mut reader = output.read();
        assert!(poll(&mut reader, waker).is_pending());
        readers.push(reader);
    }
    sender.try_send(vec![1; 40 * 1024]).unwrap();
    sender.try_send(vec![2; 40 * 1024]).unwrap();
    sender.try_send(vec![3]).unwrap();
    let mut current = output.read();
    for reader in &mut readers {
        assert_eq!(
            poll(reader, waker),
            Poll::Ready(Err("PTY read superseded".to_string()))
        );
    }
    assert_eq!(
        poll(&mut current, waker),
        Poll::Ready(Ok([vec![1; 40 * 1024], vec![2; 40 * 1024]].concat()))
    );
    drop(current);
    let mut next = output.read();
    assert_eq!(poll(&mut next, waker), Poll::Ready(Ok(vec![3])));
    drop(next);
    drop(sender);
    let mut end = output.read();
    assert_eq!(
        poll(&mut end, waker),
        Poll::Ready(Err("PTY closed".to_string()))
    );
}

#[test]
fn close_wakes_parked_reader_and_keeps_backend_ownership() {
    let (output, _sender) = output();
    let wakes = Arc::new(WakeCounter::default());
    let waker = Waker::from(wakes.clone());
    let mut reader = output.read();
    assert!(poll(&mut reader, &waker).is_pending());
    output.close();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert!(output.is_configured());
    assert_eq!(
        poll(&mut reader, &waker),
        Poll::Ready(Err("PTY closed".to_string()))
    );
}

#[test]
fn installation_wakes_a_request_waiting_for_the_channel() {
    let output = BackendOutput::default();
    let wakes = Arc::new(WakeCounter::default());
    let waker = Waker::from(wakes.clone());
    let mut reader = output.read();
    assert!(poll(&mut reader, &waker).is_pending());
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    output.install(receiver);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    sender.try_send(vec![7]).unwrap();
    assert_eq!(poll(&mut reader, &waker), Poll::Ready(Ok(vec![7])));
}
