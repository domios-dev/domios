use std::io::Write;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{sync_channel, TrySendError},
    Arc,
};

pub const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Clone, Default)]
pub struct LogDropCounter {
    total: Arc<AtomicU64>,
}

impl LogDropCounter {
    fn record(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    fn report(&self, writer: &mut impl Write, name: &str, reported: &mut u64) {
        let total = self.total();
        let dropped = total.saturating_sub(*reported);
        if dropped > 0
            && writer
                .write_all(format!("[log] dropped {dropped} records sink={name}\n").as_bytes())
                .is_ok()
        {
            *reported = total;
        }
    }
}

#[derive(Clone)]
pub struct NonBlockingLog {
    sender: std::sync::mpsc::SyncSender<Arc<[u8]>>,
    drops: LogDropCounter,
}

impl NonBlockingLog {
    pub fn spawn(
        name: &'static str,
        mut writer: impl Write + Send + 'static,
        capacity: usize,
        observed_drops: Vec<(&'static str, LogDropCounter)>,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = sync_channel::<Arc<[u8]>>(capacity);
        let drops = LogDropCounter::default();
        let worker_drops = drops.clone();
        let mut observed_drops: Vec<_> = observed_drops
            .into_iter()
            .map(|(name, counter)| (name, counter, 0))
            .collect();
        std::thread::Builder::new()
            .name(format!("log-{name}"))
            .spawn(move || {
                let mut reported = 0;
                while let Ok(record) = receiver.recv() {
                    if writer
                        .write_all(&record)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        worker_drops.record();
                    }
                    worker_drops.report(&mut writer, name, &mut reported);
                    for (sink, drops, reported) in &mut observed_drops {
                        drops.report(&mut writer, sink, reported);
                    }
                    let _ = writer.flush();
                }
            })?;
        Ok(Self { sender, drops })
    }

    pub fn submit(&self, record: Arc<[u8]>) {
        if record.len() > MAX_RECORD_BYTES {
            self.drops.record();
            return;
        }
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.sender.try_send(record)
        {
            self.drops.record();
        }
    }

    pub fn drops(&self) -> LogDropCounter {
        self.drops.clone()
    }
}

#[cfg(test)]
mod tests;
