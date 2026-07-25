//! Off-thread KPI writer. Choke points hand over an already-built event and return;
//! sqlite and provider-file parsing happen on a dedicated std thread, never on the
//! Tauri async runtime and never inside a delivery's critical section.

use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::Arc;

use super::attribution::KpiAttribution;
use super::usage::{UsageIngestor, UsageSource};
use crate::agent::kpi_ledger::{KpiEventInput, KpiLedger};

enum KpiCommand {
    Event(KpiEventInput),
    Ingest {
        source: UsageSource,
        attribution: Box<KpiAttribution>,
    },
    Drain(SyncSender<()>),
}

struct RecorderInner {
    tx: Sender<KpiCommand>,
}

/// Cheap to clone; a disabled recorder holds no channel, no thread and no db file.
#[derive(Clone, Default)]
pub struct KpiRecorder {
    inner: Option<Arc<RecorderInner>>,
}

impl KpiRecorder {
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Opens (and creates) the ledger only when the flag is on — a disabled instance
    /// touches no disk at all.
    pub fn from_config(
        config: &crate::config::schema::AppConfig,
        data_dir: &std::path::Path,
    ) -> Self {
        if !crate::agent::kpi_ledger::kpi_ledger_enabled(config) {
            return Self::disabled();
        }
        match KpiLedger::open(data_dir) {
            Ok(ledger) => Self::with_ledger(Arc::new(ledger)),
            Err(e) => {
                log::warn!("[kpi] ledger open failed: {} — KPI collection disabled", e);
                Self::disabled()
            }
        }
    }

    pub fn with_ledger(ledger: Arc<KpiLedger>) -> Self {
        let (tx, rx) = channel::<KpiCommand>();
        std::thread::Builder::new()
            .name("kpi-ledger".into())
            .spawn(move || {
                let mut ingestor = UsageIngestor::default();
                let mut pending: Vec<KpiEventInput> = Vec::new();
                while let Ok(first) = rx.recv() {
                    // Whatever else is already queued joins this commit; the loop never
                    // waits for a batch to fill.
                    for cmd in std::iter::once(first).chain(rx.try_iter()) {
                        match cmd {
                            KpiCommand::Event(event) => pending.push(event),
                            KpiCommand::Ingest {
                                source,
                                attribution,
                            } => {
                                flush(&ledger, &mut pending);
                                ingest_into_ledger(&ledger, &mut ingestor, &source, &attribution);
                            }
                            KpiCommand::Drain(ack) => {
                                flush(&ledger, &mut pending);
                                let _ = ack.send(());
                            }
                        }
                    }
                    flush(&ledger, &mut pending);
                }
            })
            .expect("spawn kpi-ledger thread");
        Self {
            inner: Some(Arc::new(RecorderInner { tx })),
        }
    }

    /// Choke points must gate attribution work on this — when off, nothing is resolved.
    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub fn record(&self, event: KpiEventInput) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.send(KpiCommand::Event(event));
        }
    }

    pub fn request_ingest(&self, source: UsageSource, attribution: KpiAttribution) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.send(KpiCommand::Ingest {
                source,
                attribution: Box::new(attribution),
            });
        }
    }

    /// Blocks until every command queued before this call has been applied. The ack is an
    /// explicit ordering signal on the same FIFO channel, not a timed wait.
    pub fn drain(&self) {
        let Some(inner) = &self.inner else { return };
        let (ack_tx, ack_rx) = sync_channel::<()>(0);
        if inner.tx.send(KpiCommand::Drain(ack_tx)).is_ok() {
            let _ = ack_rx.recv();
        }
    }
}

fn flush(ledger: &KpiLedger, pending: &mut Vec<KpiEventInput>) {
    if pending.is_empty() {
        return;
    }
    if let Err(e) = ledger.insert_many(pending) {
        log::warn!(
            "[kpi] batch insert of {} events failed: {}",
            pending.len(),
            e
        );
    }
    pending.clear();
}

/// Resumes from the ledger's persisted watermark on this source's first ingest of the
/// current process — without this, a daemon restart re-reads every session file from
/// byte/rowid zero and re-inserts every already-recorded row.
fn seed_cursor_from_ledger(ledger: &KpiLedger, ingestor: &mut UsageIngestor, source: &UsageSource) {
    if ingestor.cursor_offset(source).is_some() {
        return;
    }
    let path = source.path.to_string_lossy();
    match ledger.load_usage_cursor(&path) {
        Ok(Some(offset)) => ingestor.seed_cursor(source, offset),
        Ok(None) => {}
        Err(e) => log::warn!("[kpi] usage cursor load {} failed: {}", path, e),
    }
}

fn ingest_into_ledger(
    ledger: &KpiLedger,
    ingestor: &mut UsageIngestor,
    source: &UsageSource,
    attribution: &KpiAttribution,
) {
    seed_cursor_from_ledger(ledger, ingestor, source);
    let offset_before = ingestor.cursor_offset(source);
    let batch = match ingestor.ingest(source) {
        Ok(batch) => batch,
        Err(e) => {
            log::warn!("[kpi] usage ingest {} failed: {}", source.path.display(), e);
            return;
        }
    };
    let new_offset = ingestor.cursor_offset(source).unwrap_or_default();
    if batch.is_empty() && offset_before == Some(new_offset) {
        return;
    }
    let events: Vec<KpiEventInput> = batch
        .records
        .iter()
        .map(|record| match record.tokens {
            Some(tokens) => attribution.token_event_at(tokens, record.timestamp),
            None => attribution.event_at(
                crate::agent::kpi_ledger::KpiEventKind::Compaction,
                0,
                record.timestamp,
            ),
        })
        .collect();
    if let Err(e) = ledger.insert_usage_batch(&events, &source.path.to_string_lossy(), new_offset) {
        log::warn!("[kpi] usage insert failed: {}", e);
    }
}
