use tokio::sync::mpsc::{Receiver, Sender};

pub(super) struct OutputBatch {
    pub(super) data: Vec<u8>,
    pub(super) receipts: Vec<super::pty_frame_timing::Receipt>,
}

pub(super) fn prepare_batches(
    mut input: Receiver<OutputBatch>,
    output: Sender<OutputBatch>,
    cap: usize,
    mut prepare: impl FnMut(&[u8]) -> bool,
) {
    while let Some(mut batch) = input.blocking_recv() {
        drain_feed_batch(&mut input, &mut batch, cap);
        for receipt in &batch.receipts {
            receipt.phase("feed_started", batch.data.len());
        }
        if prepare(&batch.data) && output.blocking_send(batch).is_err() {
            break;
        }
    }
}

fn drain_feed_batch(input: &mut Receiver<OutputBatch>, batch: &mut OutputBatch, cap: usize) {
    while batch.data.len() < cap {
        match input.try_recv() {
            Ok(next) => {
                batch.data.extend_from_slice(&next.data);
                batch.receipts.extend(next.receipts);
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn packet(data: &[u8]) -> OutputBatch {
        OutputBatch {
            data: data.to_vec(),
            receipts: Vec::new(),
        }
    }

    #[test]
    fn blocked_status_observer_cannot_hold_the_next_parsed_frame() {
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(3);
        let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(3);
        let (parsed_tx, parsed_rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let prepare = std::thread::spawn(move || {
            prepare_batches(input_rx, output_tx, 1, |data| {
                parsed_tx.send(data.to_vec()).unwrap();
                true
            })
        });
        let observer = std::thread::spawn(move || {
            let first = output_rx.blocking_recv().unwrap();
            entered_tx.send(()).unwrap();
            let _ = release_rx.recv();
            let mut data = first.data;
            while let Some(next) = output_rx.blocking_recv() {
                data.extend(next.data);
            }
            data
        });
        assert!(input_tx.blocking_send(packet(b"first")).is_ok());
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            parsed_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            b"first"
        );
        assert!(input_tx.blocking_send(packet(b"cursor")).is_ok());
        let parsed = parsed_rx.recv_timeout(Duration::from_secs(5));
        drop(input_tx);
        release_tx.send(()).unwrap();
        prepare.join().unwrap();
        assert_eq!(observer.join().unwrap(), b"firstcursor");
        assert_eq!(parsed.unwrap(), b"cursor");
    }

    #[test]
    fn rejected_generation_is_not_forwarded_to_status_work() {
        let (input_tx, input_rx) = tokio::sync::mpsc::channel(2);
        let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(2);
        assert!(input_tx.blocking_send(packet(b"old")).is_ok());
        assert!(input_tx.blocking_send(packet(b"current")).is_ok());
        drop(input_tx);
        prepare_batches(input_rx, output_tx, 1, |data| data == b"current");
        assert_eq!(output_rx.blocking_recv().unwrap().data, b"current");
        assert!(output_rx.blocking_recv().is_none());
    }
    #[test]
    fn traced_feed_batch_preserves_bytes_and_read_receipts() {
        let _scope = crate::service::pty_frame_timing::Scope::begin("batch-test-pty", "tab");
        let (tx, mut rx) = tokio::sync::mpsc::channel(3);
        for data in [b"first".to_vec(), b"second".to_vec(), b"third".to_vec()] {
            let receipt = crate::service::pty_frame_timing::Receipt::read_completed(
                "batch-test-pty",
                data.len(),
            )
            .unwrap();
            assert!(tx
                .try_send(super::OutputBatch {
                    data,
                    receipts: vec![receipt]
                })
                .is_ok());
        }
        let mut batch = rx.blocking_recv().unwrap();
        super::drain_feed_batch(&mut rx, &mut batch, 6);
        assert_eq!(batch.data, b"firstsecond");
        assert_eq!(batch.receipts.len(), 2);
        assert_eq!(rx.try_recv().unwrap().data, b"third");
    }

    #[test]
    fn resize_burst_is_drained_as_one_available_feed_batch() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(300);
        for _ in 0..269 {
            tx.try_send(super::OutputBatch {
                data: vec![b'x'; 112],
                receipts: Vec::new(),
            })
            .ok()
            .unwrap();
        }
        drop(tx);

        let mut data = rx.blocking_recv().unwrap();
        super::drain_feed_batch(&mut rx, &mut data, 256 * 1024);

        assert_eq!(data.data.len(), 269 * 112);
        assert!(rx.blocking_recv().is_none());
    }
}
