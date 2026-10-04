mod mailbox;
mod pins;
mod support;

use crate::assets::{claim_root, AssetId, AssetRepository, Layout, RootClaim};
use crate::error::{io_error, sqlite_error, StorageError};
use crate::repositories::workspaces::WorkspaceRepository;
use crate::repositories::{
    completion, CommitReceipt, CommitTimings, Committed, Completion, Read, Revision, Settle,
    ShutdownReceipt, StorageMetrics,
};
use crate::schema::{self, EffectiveSettings};
use mailbox::Mailbox;
use pins::{ReadPins, ReleaseQueue};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
#[cfg(feature = "test-support")]
pub use support::{Fault, Hold, ReentrancyProbe};
pub(crate) use support::{FaultPoint, Faults, ReadGates};

pub const DEFAULT_MAILBOX_COMMANDS: usize = 128;
pub const DEFAULT_MAILBOX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_READ_CONNECTIONS: usize = 2;
const MAX_GROUP: usize = 64;
const MAX_REVISION_WAITERS: usize = 256;

// Every Mutex in this crate is a leaf: no path holds two, and completions are
// settled only after the owning guard is dropped.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

thread_local! {
    static OWNER_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Owner threads must never call back into Storage: a nested wait would self-deadlock.
pub(crate) fn on_owner_thread() -> bool {
    OWNER_THREAD.with(Cell::get)
}

fn mark_owner_thread() {
    OWNER_THREAD.with(|flag| flag.set(true));
}

/// Admission, connection and asset bounds of one opened instance root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageConfig {
    pub mailbox_commands: usize,
    pub mailbox_bytes: usize,
    pub read_queue: usize,
    pub read_connections: usize,
    pub busy_timeout: Duration,
    pub max_asset_bytes: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            mailbox_commands: DEFAULT_MAILBOX_COMMANDS,
            mailbox_bytes: DEFAULT_MAILBOX_BYTES,
            read_queue: DEFAULT_MAILBOX_COMMANDS,
            read_connections: MAX_READ_CONNECTIONS,
            busy_timeout: Duration::from_secs(5),
            max_asset_bytes: DEFAULT_MAILBOX_BYTES / 2,
        }
    }
}

impl StorageConfig {
    fn validate(&self) -> Result<(), StorageError> {
        let reason =
            if self.mailbox_commands == 0 || self.read_queue == 0 || self.mailbox_bytes == 0 {
                Some("queues need a positive bound")
            } else if self.read_connections == 0 || self.read_connections > MAX_READ_CONNECTIONS {
                Some("read connections must be 1 or 2")
            } else if self.busy_timeout.is_zero() || self.busy_timeout > Duration::from_secs(60) {
                Some("busy timeout must be within (0, 60s]")
            } else if self.max_asset_bytes > self.mailbox_bytes {
                Some("an asset must fit the mailbox byte bound")
            } else {
                None
            };
        reason.map_or(Ok(()), |reason| Err(StorageError::InvalidConfig { reason }))
    }
}

pub(crate) struct WriteCx<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) revision: Revision,
    pub(crate) layout: &'a Layout,
    pub(crate) faults: &'a mut Faults,
    pub(crate) max_asset_bytes: usize,
    pub(crate) rows_written: u64,
    pub(crate) released: Vec<AssetId>,
}

impl WriteCx<'_> {
    /// Executes one domain row statement and counts the rows it wrote.
    pub(crate) fn write(
        &mut self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<usize, StorageError> {
        let changed = self
            .conn
            .prepare_cached(sql)
            .and_then(|mut statement| statement.execute(params))
            .map_err(|e| sqlite_error("write row", e))?;
        self.rows_written += changed as u64;
        Ok(changed)
    }
}

pub(crate) struct WriterEnv<'a> {
    pub(crate) conn: &'a mut Connection,
    pub(crate) layout: &'a Layout,
    pub(crate) faults: &'a mut Faults,
    pub(crate) releases: &'a mut ReleaseQueue,
}

pub(crate) struct ReadCx<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) layout: &'a Layout,
    gates: &'a ReadGates,
}

impl ReadCx<'_> {
    pub(crate) fn before_asset_file(&self) {
        self.gates.before_asset_file();
    }
}

/// What a reader thread owns besides its connection.
struct ReaderContext {
    layout: Layout,
    pins: Arc<ReadPins>,
    published: Arc<Published>,
    writes: Arc<Mailbox<WriteJob>>,
    gates: Arc<ReadGates>,
}

/// Registered before the snapshot opens and released after it ends.
struct SnapshotPin<'a> {
    context: &'a ReaderContext,
    token: u64,
}

impl Drop for SnapshotPin<'_> {
    fn drop(&mut self) {
        if self.context.pins.leave(self.token) {
            self.context.writes.signal();
        }
    }
}

pub(crate) trait MutationJob: Send {
    fn apply(&mut self, cx: &mut WriteCx<'_>) -> Result<(), StorageError>;
    fn settle(self: Box<Self>, outcome: Result<CommitReceipt, StorageError>);
}

pub(crate) trait ExclusiveJob: Send {
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>);
    fn fail(self: Box<Self>, error: StorageError);
}

trait ReadJob: Send {
    fn run(self: Box<Self>, conn: &mut Connection, context: &ReaderContext);
    fn fail(self: Box<Self>, error: StorageError);
}

pub(crate) enum WriteJob {
    Mutation(Box<dyn MutationJob>),
    Exclusive(Box<dyn ExclusiveJob>),
}

impl WriteJob {
    fn is_mutation(&self) -> bool {
        matches!(self, WriteJob::Mutation(_))
    }

    fn fail(self, error: StorageError) {
        match self {
            WriteJob::Mutation(job) => job.settle(Err(error)),
            WriteJob::Exclusive(job) => job.fail(error),
        }
    }
}

struct TypedMutation<T, F> {
    op: Option<F>,
    value: Option<T>,
    settle: Settle<Committed<T>>,
}

impl<T, F> MutationJob for TypedMutation<T, F>
where
    T: Send,
    F: FnOnce(&mut WriteCx<'_>) -> Result<T, StorageError> + Send,
{
    fn apply(&mut self, cx: &mut WriteCx<'_>) -> Result<(), StorageError> {
        let op = self.op.take().ok_or(StorageError::Unconfirmed {
            operation: "repeated mutation",
        })?;
        self.value = Some(op(cx)?);
        Ok(())
    }

    fn settle(self: Box<Self>, outcome: Result<CommitReceipt, StorageError>) {
        let TypedMutation { value, settle, .. } = *self;
        settle.settle(match (outcome, value) {
            (Ok(receipt), Some(value)) => Ok(Committed { value, receipt }),
            (Ok(_), None) => Err(StorageError::Unconfirmed {
                operation: "mutation",
            }),
            (Err(error), _) => Err(error),
        });
    }
}

struct TypedExclusive<T, F> {
    op: F,
    settle: Settle<T>,
}

impl<T, F> ExclusiveJob for TypedExclusive<T, F>
where
    T: Send,
    F: FnOnce(&mut WriterEnv<'_>) -> Result<T, StorageError> + Send,
{
    fn run(self: Box<Self>, env: &mut WriterEnv<'_>) {
        let TypedExclusive { op, settle } = *self;
        settle.settle(op(env));
    }

    fn fail(self: Box<Self>, error: StorageError) {
        let TypedExclusive { settle, .. } = *self;
        settle.settle(Err(error));
    }
}

struct TypedRead<T, F> {
    op: F,
    settle: Settle<Read<T>>,
}

impl<T, F> ReadJob for TypedRead<T, F>
where
    T: Send,
    F: FnOnce(&ReadCx<'_>) -> Result<T, StorageError> + Send,
{
    fn run(self: Box<Self>, conn: &mut Connection, context: &ReaderContext) {
        let TypedRead { op, settle } = *self;
        settle.settle(read_snapshot(conn, context, op));
    }

    fn fail(self: Box<Self>, error: StorageError) {
        let TypedRead { settle, .. } = *self;
        settle.settle(Err(error));
    }
}

/// The pin outlives the transaction, so it is released before the caller is settled.
fn read_snapshot<T>(
    conn: &mut Connection,
    context: &ReaderContext,
    op: impl FnOnce(&ReadCx<'_>) -> Result<T, StorageError>,
) -> Result<Read<T>, StorageError> {
    let _pin = SnapshotPin {
        token: context.pins.enter(&context.published.revision),
        context,
    };
    let tx = conn
        .transaction()
        .map_err(|e| sqlite_error("begin read", e))?;
    let revision = Revision(schema::read_meta(&tx, schema::COMMIT_REVISION)?);
    let cx = ReadCx {
        conn: &tx,
        layout: &context.layout,
        gates: &context.gates,
    };
    let value = op(&cx)?;
    tx.commit().map_err(|e| sqlite_error("end read", e))?;
    Ok(Read { value, revision })
}

#[derive(Default)]
struct Waiters {
    closed: bool,
    pending: Vec<(u64, Settle<Revision>)>,
}

/// Post-commit publication: the revision moves only after COMMIT, then waiters wake.
struct Published {
    revision: AtomicU64,
    waiters: Mutex<Waiters>,
}

impl Published {
    fn store(&self, revision: u64) {
        self.revision.store(revision, Ordering::Release);
    }

    fn wake(&self, revision: u64) {
        let ready: Vec<_> = {
            let mut waiters = lock(&self.waiters);
            let (ready, keep) = std::mem::take(&mut waiters.pending)
                .into_iter()
                .partition(|(known, _)| *known < revision);
            waiters.pending = keep;
            ready
        };
        for (_, settle) in ready {
            settle.settle(Ok(Revision(revision)));
        }
    }

    fn close(&self) {
        let pending = {
            let mut waiters = lock(&self.waiters);
            waiters.closed = true;
            std::mem::take(&mut waiters.pending)
        };
        for (_, settle) in pending {
            settle.settle(Err(StorageError::Closed));
        }
    }
}

#[derive(Default)]
struct LifecycleState {
    outcome: Option<Result<ShutdownReceipt, StorageError>>,
    waiters: Vec<Settle<ShutdownReceipt>>,
}

#[derive(Default)]
struct Lifecycle {
    state: Mutex<LifecycleState>,
}

impl Lifecycle {
    fn finish(&self, outcome: Result<ShutdownReceipt, StorageError>) {
        let waiters = {
            let mut state = lock(&self.state);
            state.outcome.get_or_insert_with(|| outcome.clone());
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            waiter.settle(outcome.clone());
        }
    }
}

struct Shared {
    writes: Arc<Mailbox<WriteJob>>,
    reads: Arc<Mailbox<Box<dyn ReadJob>>>,
    published: Arc<Published>,
    lifecycle: Arc<Lifecycle>,
    metrics: Arc<Mutex<StorageMetrics>>,
    #[cfg(feature = "test-support")]
    gates: Arc<ReadGates>,
    settings: EffectiveSettings,
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.writes.close();
        self.reads.close();
    }
}

/// Handle to the single owner of one instance root's `state.db`; clones share it.
#[derive(Clone)]
pub struct Storage {
    shared: Arc<Shared>,
}

impl Storage {
    /// Blocking: call off the async runtime and outside mux/ledger locks. Refuses
    /// incompatible or corrupt state instead of starting empty.
    pub fn open(root: impl AsRef<Path>, config: StorageConfig) -> Result<Storage, StorageError> {
        if on_owner_thread() {
            return Err(StorageError::Reentrant);
        }
        config.validate()?;
        let claim = claim_root(root.as_ref())?;
        let (writer_conn, writer_settings) =
            schema::open_writer(&claim.layout, &claim.root, config.busy_timeout)?;
        let revision = schema::read_meta(&writer_conn, schema::COMMIT_REVISION)?;
        let mut reader_conns = Vec::with_capacity(config.read_connections);
        for _ in 0..config.read_connections {
            reader_conns.push(schema::open_reader(&claim.layout, config.busy_timeout)?);
        }
        let settings = EffectiveSettings {
            writer: writer_settings,
            readers: reader_conns
                .iter()
                .map(|(_, settings)| settings.clone())
                .collect(),
        };
        let writes = Arc::new(Mailbox::new(config.mailbox_commands, config.mailbox_bytes));
        let reads: Arc<Mailbox<Box<dyn ReadJob>>> =
            Arc::new(Mailbox::new(config.read_queue, usize::MAX));
        let published = Arc::new(Published {
            revision: AtomicU64::new(revision),
            waiters: Mutex::default(),
        });
        let lifecycle = Arc::new(Lifecycle::default());
        let metrics = Arc::new(Mutex::new(StorageMetrics::default()));
        let (pins, gates) = (
            Arc::new(ReadPins::default()),
            Arc::new(ReadGates::default()),
        );
        let guard = ExitGuard {
            writes: writes.clone(),
            reads: reads.clone(),
            published: published.clone(),
            lifecycle: lifecycle.clone(),
            armed: true,
        };
        let mut readers = Vec::with_capacity(reader_conns.len());
        for (index, (conn, _)) in reader_conns.into_iter().enumerate() {
            let context = ReaderContext {
                layout: claim.layout.clone(),
                pins: pins.clone(),
                published: published.clone(),
                writes: writes.clone(),
                gates: gates.clone(),
            };
            let queue = reads.clone();
            readers.push(spawn(format!("storage-reader-{index}"), move || {
                reader_main(conn, context, queue)
            })?);
        }
        let writer = Writer {
            conn: writer_conn,
            claim,
            revision,
            faults: Faults::default(),
            releases: ReleaseQueue::new(pins),
            max_asset_bytes: config.max_asset_bytes,
            readers,
            writes: writes.clone(),
            reads: reads.clone(),
            published: published.clone(),
            metrics: metrics.clone(),
            drained: 0,
        };
        spawn("storage-writer".to_string(), move || {
            writer_main(writer, guard)
        })?;
        Ok(Storage {
            shared: Arc::new(Shared {
                writes,
                reads,
                published,
                lifecycle,
                metrics,
                #[cfg(feature = "test-support")]
                gates,
                settings,
            }),
        })
    }

    pub fn workspaces(&self) -> WorkspaceRepository {
        WorkspaceRepository::new(self.clone())
    }

    pub fn assets(&self) -> AssetRepository {
        AssetRepository::new(self.clone())
    }

    /// Latest durably committed revision; it moves only after COMMIT returned.
    pub fn revision(&self) -> Revision {
        Revision(self.shared.published.revision.load(Ordering::Acquire))
    }

    /// Settles with the first committed revision newer than `known`.
    pub fn committed_after(&self, known: Revision) -> Completion<Revision> {
        let published = &self.shared.published;
        let ready = {
            let mut waiters = lock(&published.waiters);
            let current = published.revision.load(Ordering::Acquire);
            if current > known.0 {
                Ok(Revision(current))
            } else if waiters.closed {
                Err(StorageError::Closed)
            } else if waiters.pending.len() >= MAX_REVISION_WAITERS {
                Err(StorageError::Backpressure {
                    queued_commands: waiters.pending.len(),
                    queued_bytes: 0,
                    limit_commands: MAX_REVISION_WAITERS,
                    limit_bytes: 0,
                })
            } else {
                let (settle, completion) = completion();
                waiters.pending.push((known.0, settle));
                return completion;
            }
        };
        Completion::settled(ready)
    }

    pub fn effective_settings(&self) -> EffectiveSettings {
        self.shared.settings.clone()
    }

    pub fn metrics(&self) -> StorageMetrics {
        lock(&self.shared.metrics).clone()
    }

    /// Closes admission now; accepted work drains, then handles and the root are released.
    pub fn shutdown(&self) -> Completion<ShutdownReceipt> {
        if on_owner_thread() {
            return Completion::settled(Err(StorageError::Reentrant));
        }
        self.shared.writes.close();
        self.shared.reads.close();
        let finished = {
            let mut state = lock(&self.shared.lifecycle.state);
            match state.outcome.clone() {
                Some(outcome) => outcome,
                None => {
                    let (settle, completion) = completion();
                    state.waiters.push(settle);
                    return completion;
                }
            }
        };
        Completion::settled(finished)
    }

    pub(crate) fn submit_mutation<T, F>(&self, cost: usize, op: F) -> Completion<Committed<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut WriteCx<'_>) -> Result<T, StorageError> + Send + 'static,
    {
        let (settle, completion) = completion();
        self.admit(
            WriteJob::Mutation(Box::new(TypedMutation {
                op: Some(op),
                value: None,
                settle,
            })),
            cost,
        );
        completion
    }

    pub(crate) fn submit_exclusive<T, F>(&self, cost: usize, op: F) -> Completion<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut WriterEnv<'_>) -> Result<T, StorageError> + Send + 'static,
    {
        let (settle, completion) = completion();
        self.admit(
            WriteJob::Exclusive(Box::new(TypedExclusive { op, settle })),
            cost,
        );
        completion
    }

    pub(crate) fn submit_read<T, F>(&self, op: F) -> Completion<Read<T>>
    where
        T: Send + 'static,
        F: FnOnce(&ReadCx<'_>) -> Result<T, StorageError> + Send + 'static,
    {
        let (settle, completion) = completion();
        let job: Box<dyn ReadJob> = Box::new(TypedRead { op, settle });
        if on_owner_thread() {
            job.fail(StorageError::Reentrant);
        } else if let Err((job, error)) = self.shared.reads.push(job, 0) {
            self.note_rejection();
            job.fail(error);
        }
        completion
    }

    fn admit(&self, job: WriteJob, cost: usize) {
        if on_owner_thread() {
            return job.fail(StorageError::Reentrant);
        }
        if let Err((job, error)) = self.shared.writes.push(job, cost) {
            self.note_rejection();
            job.fail(error);
        }
    }

    fn note_rejection(&self) {
        lock(&self.shared.metrics).rejected_admissions += 1;
    }
}

fn spawn(
    name: String,
    body: impl FnOnce() + Send + 'static,
) -> Result<JoinHandle<()>, StorageError> {
    std::thread::Builder::new()
        .name(name)
        .spawn(body)
        .map_err(|e| io_error("spawn storage thread", e))
}

struct ReaderGuard(Arc<Mailbox<Box<dyn ReadJob>>>);

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            for queued in self.0.close_and_take() {
                queued.job.fail(StorageError::OwnerStopped);
            }
        }
    }
}

fn reader_main(
    mut conn: Connection,
    context: ReaderContext,
    reads: Arc<Mailbox<Box<dyn ReadJob>>>,
) {
    mark_owner_thread();
    let _guard = ReaderGuard(reads.clone());
    while let Some(popped) = reads.pop(1, |_| false) {
        for queued in popped.jobs {
            queued.job.run(&mut conn, &context);
        }
    }
    let _ = conn.close();
}

/// Settles everything the writer can no longer run, including after a panic.
struct ExitGuard {
    writes: Arc<Mailbox<WriteJob>>,
    reads: Arc<Mailbox<Box<dyn ReadJob>>>,
    published: Arc<Published>,
    lifecycle: Arc<Lifecycle>,
    armed: bool,
}

impl ExitGuard {
    fn complete(mut self, outcome: Result<ShutdownReceipt, StorageError>) {
        self.armed = false;
        self.settle(outcome);
    }

    fn settle(&self, outcome: Result<ShutdownReceipt, StorageError>) {
        for queued in self.writes.close_and_take() {
            queued.job.fail(StorageError::OwnerStopped);
        }
        for queued in self.reads.close_and_take() {
            queued.job.fail(StorageError::OwnerStopped);
        }
        self.published.close();
        self.lifecycle.finish(outcome);
    }
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            self.settle(Err(StorageError::OwnerStopped));
        }
    }
}

struct Applied {
    revision: Revision,
    rows_written: u64,
    released: Vec<AssetId>,
}

enum GroupFailure {
    RolledBack(StorageError),
    Commit { error: StorageError, next: u64 },
}

struct Writer {
    conn: Connection,
    claim: RootClaim,
    revision: u64,
    faults: Faults,
    releases: ReleaseQueue,
    max_asset_bytes: usize,
    readers: Vec<JoinHandle<()>>,
    writes: Arc<Mailbox<WriteJob>>,
    reads: Arc<Mailbox<Box<dyn ReadJob>>>,
    published: Arc<Published>,
    metrics: Arc<Mutex<StorageMetrics>>,
    drained: u64,
}

fn writer_main(writer: Writer, guard: ExitGuard) {
    mark_owner_thread();
    let outcome = writer.run();
    guard.complete(outcome);
}

impl Writer {
    fn run(mut self) -> Result<ShutdownReceipt, StorageError> {
        while let Some(popped) = self.writes.pop(MAX_GROUP, WriteJob::is_mutation) {
            if popped.signaled {
                self.drain_releases();
            }
            if popped.closed {
                self.drained += popped.jobs.len() as u64;
            }
            let dequeued = Instant::now();
            let mut group = Vec::new();
            for queued in popped.jobs {
                let waited = dequeued.saturating_duration_since(queued.enqueued);
                match queued.job {
                    WriteJob::Exclusive(job) => job.run(&mut WriterEnv {
                        conn: &mut self.conn,
                        layout: &self.claim.layout,
                        faults: &mut self.faults,
                        releases: &mut self.releases,
                    }),
                    WriteJob::Mutation(job) => group.push((job, waited)),
                }
            }
            if !group.is_empty() {
                self.commit_group(group);
            }
        }
        self.finish()
    }

    /// A removal failure stays recorded in `asset_cleanup` for an explicit retry.
    fn drain_releases(&mut self) {
        let _ = self
            .releases
            .drain(&self.conn, &self.claim.layout, &mut self.faults);
    }

    /// One IMMEDIATE transaction, one savepoint per operation; acknowledgments follow COMMIT.
    fn commit_group(&mut self, mut jobs: Vec<(Box<dyn MutationJob>, Duration)>) {
        let started = Instant::now();
        let mut applied = Vec::with_capacity(jobs.len());
        let outcome = self.run_transaction(&mut jobs, &mut applied);
        let finished = Instant::now();
        let group_size = jobs.len() as u32;
        let queue_wait = jobs
            .iter()
            .map(|(_, waited)| *waited)
            .max()
            .unwrap_or_default();
        let (next, commit_started) = match outcome {
            Ok(done) => done,
            Err(failure) => return self.fail_group(jobs, applied, failure),
        };
        let timings = CommitTimings {
            queue_wait,
            transaction: commit_started.saturating_duration_since(started),
            commit: finished.saturating_duration_since(commit_started),
            group_size,
        };
        self.revision = next;
        // Publishing before the pin check keeps every later reader's floor at or above `next`.
        self.published.store(next);
        let released: Vec<(AssetId, u64)> = applied
            .iter()
            .flatten()
            .flat_map(|done| {
                done.released
                    .iter()
                    .map(move |asset| (asset.clone(), done.revision.0))
            })
            .collect();
        if !released.is_empty() {
            self.releases.defer(released);
            self.drain_releases();
        }
        let committed = applied.iter().filter(|result| result.is_ok()).count() as u64;
        self.record(committed, group_size as u64 - committed, Some(timings));
        for ((job, waited), result) in jobs.into_iter().zip(applied) {
            let timings = CommitTimings {
                queue_wait: waited,
                ..timings
            };
            job.settle(result.map(|done| CommitReceipt {
                revision: done.revision,
                rows_written: done.rows_written,
                timings,
            }));
        }
        self.published.wake(next);
    }

    fn run_transaction(
        &mut self,
        jobs: &mut [(Box<dyn MutationJob>, Duration)],
        applied: &mut Vec<Result<Applied, StorageError>>,
    ) -> Result<(u64, Instant), GroupFailure> {
        let layout = &self.claim.layout;
        let mut tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| GroupFailure::RolledBack(sqlite_error("begin write", e)))?;
        let mut next = self.revision;
        for (job, _) in jobs.iter_mut() {
            match apply_one(
                &mut tx,
                job.as_mut(),
                Revision(next + 1),
                layout,
                &mut self.faults,
                self.max_asset_bytes,
            ) {
                Ok(done) => {
                    next += 1;
                    applied.push(Ok(done));
                }
                Err(error) => {
                    applied.push(Err(error.clone()));
                    if tx.is_autocommit() {
                        return Err(GroupFailure::RolledBack(error));
                    }
                }
            }
        }
        if next > self.revision {
            schema::write_meta(&tx, schema::COMMIT_REVISION, next)
                .map_err(GroupFailure::RolledBack)?;
        }
        if self.faults.take(FaultPoint::Commit) {
            return Err(GroupFailure::RolledBack(StorageError::Injected {
                point: "commit",
            }));
        }
        let commit_started = Instant::now();
        tx.commit().map_err(|e| GroupFailure::Commit {
            error: sqlite_error("commit", e),
            next,
        })?;
        Ok((next, commit_started))
    }

    fn fail_group(
        &mut self,
        jobs: Vec<(Box<dyn MutationJob>, Duration)>,
        applied: Vec<Result<Applied, StorageError>>,
        failure: GroupFailure,
    ) {
        let shared = match failure {
            GroupFailure::RolledBack(error) => error,
            GroupFailure::Commit { error, next } => self.reconcile_commit_failure(error, next),
        };
        self.record(0, jobs.len() as u64, None);
        let mut own = applied.into_iter();
        for (job, _) in jobs {
            let error = match own.next() {
                Some(Err(error)) => error,
                _ => shared.clone(),
            };
            job.settle(Err(error));
        }
    }

    /// A failed COMMIT is reconciled by the exact stored revision, never guessed.
    fn reconcile_commit_failure(&mut self, error: StorageError, next: u64) -> StorageError {
        match schema::read_meta(&self.conn, schema::COMMIT_REVISION) {
            Ok(stored) if stored == next && stored > self.revision => {
                self.revision = stored;
                self.published.store(stored);
                self.published.wake(stored);
                StorageError::Unconfirmed {
                    operation: "commit",
                }
            }
            Ok(_) => StorageError::CommitFailed {
                detail: error.to_string(),
            },
            Err(_) => StorageError::Unconfirmed {
                operation: "commit",
            },
        }
    }

    fn record(&self, committed: u64, failed: u64, timings: Option<CommitTimings>) {
        let mut metrics = lock(&self.metrics);
        metrics.committed_operations += committed;
        metrics.failed_operations += failed;
        metrics.commit_groups += 1;
        if let Some(timings) = timings {
            metrics.max_queue_wait = metrics.max_queue_wait.max(timings.queue_wait);
            metrics.max_transaction = metrics.max_transaction.max(timings.transaction);
            metrics.max_commit = metrics.max_commit.max(timings.commit);
            metrics.last_group = Some(timings);
        }
    }

    /// Readers close first: their pins end, deferred removals complete, and the final
    /// checkpoint can truncate so SQLite removes WAL/SHM before the root lock is released.
    fn finish(self) -> Result<ShutdownReceipt, StorageError> {
        let Writer {
            conn,
            claim,
            revision,
            mut faults,
            mut releases,
            readers,
            reads,
            drained,
            ..
        } = self;
        reads.close();
        for reader in readers {
            let _ = reader.join();
        }
        let _ = releases.drain(&conn, &claim.layout, &mut faults);
        let checkpointed = schema::checkpoint_truncate(&conn);
        let closed = conn
            .close()
            .map_err(|(_, e)| sqlite_error("close database", e));
        drop(claim);
        closed?;
        Ok(ShutdownReceipt {
            drained_writes: drained,
            revision: Revision(revision),
            checkpointed: checkpointed?,
        })
    }
}

fn apply_one(
    tx: &mut Transaction<'_>,
    job: &mut dyn MutationJob,
    revision: Revision,
    layout: &Layout,
    faults: &mut Faults,
    max_asset_bytes: usize,
) -> Result<Applied, StorageError> {
    let savepoint = tx
        .savepoint()
        .map_err(|e| sqlite_error("begin operation", e))?;
    let mut cx = WriteCx {
        conn: &savepoint,
        revision,
        layout,
        faults,
        max_asset_bytes,
        rows_written: 0,
        released: Vec::new(),
    };
    job.apply(&mut cx)?;
    let applied = Applied {
        revision,
        rows_written: cx.rows_written,
        released: cx.released,
    };
    savepoint
        .commit()
        .map_err(|e| sqlite_error("release operation", e))?;
    Ok(applied)
}
