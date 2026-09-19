//! Off-thread KPI writer. Choke points hand over an already-built event and return;
//! sqlite and provider-file parsing happen on a dedicated std thread, never on the
//! Tauri async runtime and never inside a delivery's critical section.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Arc, OnceLock};

use super::attribution::KpiAttribution;
use super::usage::{UsageCursor, UsageIngestor, UsageRecord, UsageSource};
use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind, KpiLedger, UsageRebuildOutcome};

type RebuildReply = SyncSender<Result<UsageRebuildOutcome, String>>;

enum KpiCommand {
    Event(KpiEventInput),
    Ingest {
        source: UsageSource,
        attribution: Box<KpiAttribution>,
    },
    RebuildUsage(RebuildReply),
    Drain(SyncSender<()>),
}

/// Receives the distinct plan ids of one committed batch; unattributed rows add none.
pub type CommitListener = Box<dyn Fn(Vec<String>) + Send + Sync>;

struct RecorderInner {
    tx: Sender<KpiCommand>,
    on_commit: Arc<OnceLock<CommitListener>>,
    /// Read side for callers that project ledger aggregates; writes stay on the thread.
    ledger: Arc<KpiLedger>,
}

/// Cheap to clone; a disabled recorder holds no channel, no thread and no db file.
#[derive(Clone, Default)]
pub struct KpiRecorder {
    inner: Option<Arc<RecorderInner>>,
    /// Set when the flag was on but the ledger would not open, so a not-recording
    /// surface reports that failure instead of telling the owner to turn on a live setting.
    open_error: Option<Arc<str>>,
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
                Self {
                    inner: None,
                    open_error: Some(Arc::from(e)),
                }
            }
        }
    }

    /// True only for the configured-on-but-unopenable case; an opt-out reads false.
    pub fn open_failed(&self) -> bool {
        self.open_error.is_some()
    }

    /// The host's own text for why a configured ledger did not open.
    pub fn open_error(&self) -> Option<&str> {
        self.open_error.as_deref()
    }

    pub fn with_ledger(ledger: Arc<KpiLedger>) -> Self {
        let reader = Arc::clone(&ledger);
        let on_commit: Arc<OnceLock<CommitListener>> = Arc::default();
        let listener = Arc::clone(&on_commit);
        let (tx, rx) = channel::<KpiCommand>();
        std::thread::Builder::new()
            .name("kpi-ledger".into())
            .spawn(move || {
                let mut ingestor = UsageIngestor::default();
                let mut pending: Vec<KpiEventInput> = Vec::new();
                while let Ok(first) = rx.recv() {
                    let mut committed = Committed::default();
                    // Whatever else is already queued joins this commit; the loop never
                    // waits for a batch to fill.
                    for cmd in std::iter::once(first).chain(rx.try_iter()) {
                        match cmd {
                            KpiCommand::Event(event) => pending.push(event),
                            KpiCommand::Ingest {
                                source,
                                attribution,
                            } => {
                                flush(&ledger, &mut pending, &mut committed);
                                ingest_into_ledger(
                                    &ledger,
                                    &mut ingestor,
                                    &source,
                                    &attribution,
                                    &mut committed,
                                );
                            }
                            KpiCommand::RebuildUsage(reply) => {
                                flush(&ledger, &mut pending, &mut committed);
                                let outcome = super::rebuild::rebuild_usage(&ledger, &mut ingestor);
                                if let Ok(outcome) = &outcome {
                                    committed.touch(outcome.plans.iter().cloned());
                                }
                                let _ = reply.send(outcome);
                            }
                            KpiCommand::Drain(ack) => {
                                flush(&ledger, &mut pending, &mut committed);
                                committed.notify(&listener);
                                let _ = ack.send(());
                            }
                        }
                    }
                    flush(&ledger, &mut pending, &mut committed);
                    committed.notify(&listener);
                }
            })
            .expect("spawn kpi-ledger thread");
        Self {
            inner: Some(Arc::new(RecorderInner {
                tx,
                on_commit,
                ledger: reader,
            })),
            open_error: None,
        }
    }

    /// Installs the one commit listener; a later call is ignored.
    pub fn on_commit(&self, listener: impl Fn(Vec<String>) + Send + Sync + 'static) {
        if let Some(inner) = &self.inner {
            let _ = inner.on_commit.set(Box::new(listener));
        }
    }

    /// `None` when the flag is off — the caller then renders its not-recording state
    /// instead of falling back to any other source.
    pub fn ledger(&self) -> Option<Arc<KpiLedger>> {
        self.inner.as_ref().map(|inner| Arc::clone(&inner.ledger))
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

    /// Blocks until the ledger thread has re-derived the token rows of every recorded source.
    pub fn rebuild_usage(&self) -> Result<UsageRebuildOutcome, String> {
        let Some(inner) = &self.inner else {
            return Err("kpi ledger is not recording".to_string());
        };
        let (reply_tx, reply_rx) = sync_channel(1);
        inner
            .tx
            .send(KpiCommand::RebuildUsage(reply_tx))
            .map_err(|_| "kpi ledger thread stopped".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "kpi ledger thread stopped".to_string())?
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

#[derive(Default)]
struct Committed {
    wrote: bool,
    plans: BTreeSet<String>,
}

impl Committed {
    fn record(&mut self, events: &[KpiEventInput]) {
        self.wrote |= !events.is_empty();
        self.plans
            .extend(events.iter().filter_map(|event| event.plan_id.clone()));
    }

    fn touch(&mut self, plans: impl IntoIterator<Item = String>) {
        self.wrote = true;
        self.plans.extend(plans);
    }

    fn notify(&mut self, listener: &OnceLock<CommitListener>) {
        let committed = std::mem::take(self);
        if !committed.wrote {
            return;
        }
        if let Some(listener) = listener.get() {
            listener(committed.plans.into_iter().collect());
        }
    }
}

fn flush(ledger: &KpiLedger, pending: &mut Vec<KpiEventInput>, committed: &mut Committed) {
    if pending.is_empty() {
        return;
    }
    if let Err(e) = ledger.insert_many(pending) {
        log::warn!(
            "[kpi] batch insert of {} events failed: {}",
            pending.len(),
            e
        );
    } else {
        committed.record(pending);
    }
    pending.clear();
}

/// Resumes from the ledger's persisted watermark on this source's first ingest of the
/// current process — without this, a daemon restart re-reads every session file from
/// byte/rowid zero and re-inserts every already-recorded row.
fn seed_cursor_from_ledger(ledger: &KpiLedger, ingestor: &mut UsageIngestor, source: &UsageSource) {
    if ingestor.cursor(source).is_some() {
        return;
    }
    let key = source.key();
    match ledger.load_usage_cursor(&key) {
        Ok(Some(state)) => ingestor.seed_cursor(source, UsageCursor::from_persisted(state)),
        Ok(None) => {}
        Err(e) => log::warn!("[kpi] usage cursor load {} failed: {}", key, e),
    }
}

fn usage_event(attribution: &KpiAttribution, record: &UsageRecord) -> KpiEventInput {
    match record.tokens {
        Some(tokens) => attribution.token_event_at(tokens, record.timestamp, record.key.clone()),
        None => KpiEventInput {
            dedup_key: Some(record.key.clone()),
            ..attribution.event_at(KpiEventKind::Compaction, 0, record.timestamp)
        },
    }
}

fn ingest_into_ledger(
    ledger: &KpiLedger,
    ingestor: &mut UsageIngestor,
    source: &UsageSource,
    attribution: &KpiAttribution,
    committed: &mut Committed,
) {
    seed_cursor_from_ledger(ledger, ingestor, source);
    let cursor_before = ingestor.cursor(source);
    let batch = match ingestor.ingest(source) {
        Ok(batch) => batch,
        Err(e) => {
            log::warn!("[kpi] usage ingest {} failed: {}", source.path.display(), e);
            return;
        }
    };
    let cursor = ingestor.cursor(source).unwrap_or_default();
    if batch.is_empty() && cursor_before == Some(cursor) {
        return;
    }
    // The cursor commits past every byte this pass read, so consumed but unrecorded
    // usage is gone for good and must be logged, whether unreadable or merely empty.
    if batch.unreadable_lines > 0 {
        log::warn!(
            "[kpi] {}: {} unreadable lines in {} consumed bytes; their usage is unrecoverable",
            source.path.display(),
            batch.unreadable_lines,
            batch.consumed
        );
    } else if batch.consumed_without_recording() {
        log::debug!(
            "[kpi] {}: consumed {} bytes carrying no usage record",
            source.path.display(),
            batch.consumed
        );
    }
    let events: Vec<KpiEventInput> = batch
        .records
        .iter()
        .map(|record| usage_event(attribution, record))
        .collect();
    match ledger.insert_usage_batch(&events, &source.key(), cursor.persisted()) {
        Ok(()) => committed.record(&events),
        Err(e) => log::warn!("[kpi] usage insert failed: {}", e),
    }
}
