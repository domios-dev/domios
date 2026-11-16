use tokio::sync::mpsc::{Receiver, Sender};

pub(super) struct OutputBatch {
    pub(super) completed: Vec<tokio::sync::oneshot::Sender<()>>,
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
                batch.completed.extend(next.completed);
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests;
