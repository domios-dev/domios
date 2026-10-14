//! Sentinel persistence: the legacy JSON file until the cutover, or typed Storage selected
//! only by `Store::with_storage`, staged under the lock and committed with no lock held.

mod codec;
mod ops;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap};
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::Mutex;

use super::amend::DefinitionVersion;
use super::store::{scope_revisions, validate, ScopeKey, Sentinel, Store};
use codeterm_cron::clock::SharedClock;
#[cfg(test)]
use codeterm_util::persisted_state::StateFile;
use codeterm_storage::{
    Committed, Completion, Revision, SentinelCommit, SentinelImport, SentinelRepository, Storage,
    StorageError, StoredDefinition, StoredSentinel,
};

/// Failure bound around a commit wait only; expiry is unconfirmed, never failure.
pub const COMMIT_DEADLINE: Duration = Duration::from_secs(30);
pub const GUARD_FREE_REQUIRED: &str =
    "blueprint_store_guard_free_commit_required: settle typed Storage commits without the store lock";

pub type Mutation = Completion<Committed<SentinelCommit>>;
pub type Finish<T> =
    Box<dyn FnOnce(&mut Store, Result<SentinelCommit, Failure>) -> Result<T, String> + Send>;

/// Why the store refuses all work; never reported as an empty or idle inventory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    #[cfg(test)]
    UnsupportedFormat,
    LoadFailed,
    PersistenceFailed,
    /// A typed commit outcome is unknown; admission stays closed until reconciled.
    PersistenceUnconfirmed,
}

impl LoadFailure {
    #[cfg(test)]
    fn from_raw(raw: &str) -> Self {
        let unsupported = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|jobs| {
                jobs.iter().any(|job| {
                    job["routes"]
                        .as_object()
                        .is_some_and(|routes| routes.values().any(serde_json::Value::is_object))
                })
            });
        if unsupported {
            Self::UnsupportedFormat
        } else {
            Self::LoadFailed
        }
    }

    pub fn message(self) -> String {
        let (code, detail) = match self {
            #[cfg(test)]
            Self::UnsupportedFormat => ("blueprint_store_unsupported_format", ""),
            Self::LoadFailed => ("blueprint_store_load_failed", ""),
            Self::PersistenceFailed => ("blueprint_store_persistence_failed", ""),
            Self::PersistenceUnconfirmed => (
                "blueprint_store_persistence_failed",
                "commit outcome unconfirmed until reconciled; ",
            ),
        };
        format!("{code}: {detail}original definitions remain in place; preserve/export them and recreate through supported authoring only after operator recovery")
    }
}

#[cfg(test)]
fn decode(raw: &str) -> Result<Vec<Sentinel>, String> {
    validate(serde_json::from_str(raw).map_err(|e| e.to_string())?)
}

/// The legacy JSON file backend; it stays the live authority until the cutover.
impl Store {
    #[cfg(test)]
    pub fn new(path: PathBuf, clock: SharedClock) -> Self {
        let file = StateFile::new(path);
        let (jobs, unavailable) = match std::fs::read_to_string(file.path()) {
            Ok(raw) => match decode(&raw) {
                Ok(jobs) => {
                    codeterm_util::persisted_state::release_hold(file.path());
                    (jobs, None)
                }
                Err(_) => {
                    file.hold_for_repair();
                    (Vec::new(), Some(LoadFailure::from_raw(&raw)))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && file.held().is_none() => {
                (Vec::new(), None)
            }
            Err(_) => {
                file.hold_for_repair();
                (Vec::new(), Some(LoadFailure::LoadFailed))
            }
        };
        let committed_revisions = scope_revisions(&jobs);
        Self {
            jobs,
            active: HashMap::new(),
            wake_pending: BTreeSet::new(),
            backend: Backend::Json(file),
            clock,
            unavailable,
            admission_closed: false,
            committed_revisions,
            changes: BTreeMap::new(),
            pending_stop_notices: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn save(&mut self) -> Result<(), String> {
        self.ensure_available()?;
        self.check_existing_file()?;
        let raw = serde_json::to_string_pretty(&self.jobs).map_err(|e| e.to_string())?;
        let Backend::Json(file) = &self.backend else {
            return Err(GUARD_FREE_REQUIRED.into());
        };
        if let Err(error) = file.save(&raw) {
            file.hold_for_repair();
            self.unavailable = Some(LoadFailure::PersistenceFailed);
            for cancelled in self.active.values() {
                cancelled.store(true, Ordering::SeqCst);
            }
            return Err(format!(
                "{}; write failed: {error}",
                LoadFailure::PersistenceFailed.message()
            ));
        }
        let current = scope_revisions(&self.jobs);
        let scopes: std::collections::BTreeSet<_> = current
            .keys()
            .chain(self.committed_revisions.keys())
            .cloned()
            .collect();
        for scope in scopes {
            if current.get(&scope) != self.committed_revisions.get(&scope) {
                let revision =
                    super::identity::scope_revision(&self.jobs, &scope.0, scope.1.as_deref());
                self.changes.insert(scope, revision);
            }
        }
        self.committed_revisions = current;
        Ok(())
    }

    pub fn save_legacy(&mut self) -> Result<(), String> {
        #[cfg(test)]
        {
            self.save()
        }
        #[cfg(not(test))]
        {
            Err("legacy Sentinel JSON writer is test-only".into())
        }
    }

    #[cfg(test)]
    fn check_existing_file(&mut self) -> Result<(), String> {
        let Backend::Json(file) = &self.backend else {
            return Ok(());
        };
        let failure = match std::fs::read_to_string(file.path()) {
            Ok(raw) => decode(&raw).err().map(|_| LoadFailure::from_raw(&raw)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(LoadFailure::LoadFailed),
        };
        if let Some(failure) = failure {
            file.hold_for_repair();
            self.unavailable = Some(failure);
            for cancelled in self.active.values() {
                cancelled.store(true, Ordering::SeqCst);
            }
            return Err(failure.message());
        }
        Ok(())
    }
}

pub enum Backend {
    #[cfg(test)]
    Json(StateFile),
    Sqlite(SqliteBackend),
}

impl Backend {
    pub fn sqlite(&self) -> Option<&SqliteBackend> {
        match self {
            #[cfg(test)]
            Backend::Json(_) => None,
            Backend::Sqlite(sqlite) => Some(sqlite),
        }
    }

    pub fn sqlite_mut(&mut self) -> Option<&mut SqliteBackend> {
        match self {
            #[cfg(test)]
            Backend::Json(_) => None,
            Backend::Sqlite(sqlite) => Some(sqlite),
        }
    }

    pub fn is_json(&self) -> bool {
        #[cfg(test)]
        match self {
            Backend::Json(_) => true,
            Backend::Sqlite(_) => false,
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    pub fn held(&self) -> bool {
        #[cfg(test)]
        {
            if let Backend::Json(file) = self {
                return file.held().is_some();
            }
        }
        false
    }
}

#[derive(Clone, Copy)]
struct Known {
    revision: Revision,
    ordinal: u64,
}

/// Per-stage commit revisions fence publication, so a late older outcome never regresses
/// the cache; tombstones stop an in-flight outcome from resurrecting a removed stage.
pub struct SqliteBackend {
    pub repo: SentinelRepository,
    known: HashMap<String, Known>,
    removed: BTreeSet<String>,
    floor: Option<Revision>,
    inflight: usize,
}

pub fn history_versions(records: Vec<StoredDefinition>) -> Vec<DefinitionVersion> {
    records.into_iter().map(codec::history_version).collect()
}

/// Why a staged mutation did not commit.
pub enum Failure {
    /// Nothing was applied; admission stays open.
    Refused(String),
    /// Storage failed or could not confirm the commit; the store is now unavailable.
    Failed(String),
}

impl Failure {
    pub fn message(self) -> String {
        match self {
            Failure::Refused(message) | Failure::Failed(message) => message,
        }
    }
}

enum Stage<T> {
    Ready(Result<T, String>),
    Pending {
        completion: Mutation,
        finish: Finish<T>,
    },
}

/// An authorized mutation extracted under the store lock; settle it with no lock held.
#[must_use = "settle a staged Sentinel mutation without holding the store lock"]
pub struct Staged<T>(Stage<T>);

impl<T: Send + 'static> Staged<T> {
    pub fn ready(result: Result<T, String>) -> Self {
        Staged(Stage::Ready(result))
    }

    pub fn pending(completion: Mutation, finish: Finish<T>) -> Self {
        Staged(Stage::Pending { completion, finish })
    }

    pub fn map<U: Send + 'static>(
        self,
        map: impl FnOnce(T) -> U + Send + 'static,
    ) -> Staged<U> {
        match self.0 {
            Stage::Ready(result) => Staged::ready(result.map(map)),
            Stage::Pending { completion, finish } => Staged::pending(
                completion,
                finishing(move |store, outcome| finish(store, outcome).map(map)),
            ),
        }
    }

    /// Legacy lock-held result; the JSON backend never stages a pending commit.
    #[cfg(test)]
    pub fn into_ready(self) -> Result<T, String> {
        match self.0 {
            Stage::Ready(result) => result,
            Stage::Pending { .. } => Err(GUARD_FREE_REQUIRED.into()),
        }
    }

    pub fn settle(self, store: &Mutex<Store>) -> Result<T, String> {
        self.settle_within(store, COMMIT_DEADLINE)
    }

    /// Waits for COMMIT with no lock held, then publishes under the store lock.
    pub fn settle_within(
        self,
        store: &Mutex<Store>,
        deadline: Duration,
    ) -> Result<T, String> {
        match self.0 {
            Stage::Ready(result) => result,
            Stage::Pending { completion, finish } => {
                let outcome = completion.wait(deadline);
                let mut store = store.lock();
                let concluded = store.conclude(outcome);
                finish(&mut *store, concluded)
            }
        }
    }
}

/// Boxes a settlement step with the higher-ranked store borrow spelled out.
pub fn finishing<T>(
    finish: impl FnOnce(&mut Store, Result<SentinelCommit, Failure>) -> Result<T, String>
        + Send
        + 'static,
) -> Finish<T> {
    Box::new(finish)
}

/// Stages under the store lock, releases it, then settles.
pub fn commit<T: Send + 'static>(
    store: &Mutex<Store>,
    stage: impl FnOnce(&mut Store) -> Staged<T>,
) -> Result<T, String> {
    let staged = stage(&mut *store.lock());
    staged.settle(store)
}

fn classify(error: StorageError) -> (Option<LoadFailure>, String) {
    match error {
        StorageError::Backpressure { .. } | StorageError::Busy { .. } => {
            (None, format!("blueprint_store_busy: not applied: {error}"))
        }
        StorageError::Unconfirmed { .. } => {
            (Some(LoadFailure::PersistenceUnconfirmed), error.to_string())
        }
        error => (Some(LoadFailure::PersistenceFailed), error.to_string()),
    }
}

impl Store {
    pub fn ensure_external_commit_ready(&self) -> Result<(), String> {
        self.ensure_available()?;
        match &self.backend {
            Backend::Sqlite(backend) if backend.inflight == 0 => Ok(()),
            Backend::Sqlite(_) => Err("sentinel storage has unsettled commits".into()),
            #[cfg(test)]
            Backend::Json(_) => Err(GUARD_FREE_REQUIRED.into()),
        }
    }

    pub fn adopt_external_commit(
        &mut self,
        committed: &Committed<SentinelCommit>,
    ) -> Result<(), String> {
        if !matches!(&self.backend, Backend::Sqlite(_)) {
            return Err(GUARD_FREE_REQUIRED.into());
        }
        self.publish(committed)
    }

    /// Admits one typed mutation without waiting; refusals settle through `finish` at once.
    pub fn submit<T: Send + 'static>(
        &mut self,
        build: impl FnOnce(&SentinelRepository) -> Mutation,
        finish: Finish<T>,
    ) -> Staged<T> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(finish(self, Err(Failure::Failed(error))));
        }
        #[cfg(not(test))]
        let Backend::Sqlite(backend) = &mut self.backend;
        #[cfg(test)]
        let backend = match &mut self.backend {
            Backend::Sqlite(backend) => backend,
            #[cfg(test)]
            Backend::Json(_) => return Staged::ready(Err(GUARD_FREE_REQUIRED.into())),
        };
        let completion = build(&backend.repo);
        backend.inflight += 1;
        Staged::pending(completion, finish)
    }

    fn fail(&mut self, failure: LoadFailure, detail: &str) -> Failure {
        self.unavailable = Some(failure);
        for cancelled in self.active.values() {
            cancelled.cancel();
        }
        Failure::Failed(format!("{}; {detail}", failure.message()))
    }

    fn conclude(
        &mut self,
        outcome: Result<Committed<SentinelCommit>, StorageError>,
    ) -> Result<SentinelCommit, Failure> {
        if let Some(backend) = self.backend.sqlite_mut() {
            backend.inflight = backend.inflight.saturating_sub(1);
        }
        match outcome {
            Ok(committed) => match self.publish(&committed) {
                Ok(()) => Ok(committed.value),
                Err(error) => Err(self.fail(LoadFailure::LoadFailed, &error)),
            },
            Err(error) => match classify(error) {
                (None, message) => Err(Failure::Refused(message)),
                (Some(failure), detail) => Err(self.fail(failure, &detail)),
            },
        }
    }

    /// Adopts post-commit aggregates newer than the cache, then emits scoped invalidations.
    fn publish(&mut self, committed: &Committed<SentinelCommit>) -> Result<(), String> {
        let Store {
            jobs,
            backend,
            wake_pending,
            ..
        } = self;
        let Some(backend) = backend.sqlite_mut() else {
            return Err(GUARD_FREE_REQUIRED.into());
        };
        let mut scopes: BTreeSet<ScopeKey> = BTreeSet::new();
        for removed in &committed.value.removed {
            let id = removed.as_str();
            backend.known.remove(id);
            backend.removed.insert(id.to_string());
            if let Some(position) = jobs.iter().position(|job| job.id == id) {
                let job = jobs.remove(position);
                scopes.insert((job.workspace_id, job.team_id));
            }
            wake_pending.remove(id);
        }
        for stored in &committed.value.sentinels {
            let id = stored.record.identity.id.as_str();
            let stale = match backend.known.get(id) {
                Some(known) => known.revision >= stored.revision,
                None => {
                    backend.removed.contains(id)
                        || backend.floor.is_some_and(|floor| stored.revision <= floor)
                }
            };
            if stale {
                continue;
            }
            let job = codec::decode(stored);
            scopes.insert((job.workspace_id.clone(), job.team_id.clone()));
            match jobs.iter().position(|current| current.id == id) {
                Some(position) => {
                    let previous = &jobs[position];
                    scopes.insert((previous.workspace_id.clone(), previous.team_id.clone()));
                    jobs[position] = job;
                }
                None => {
                    let position = jobs
                        .iter()
                        .position(|current| {
                            backend
                                .known
                                .get(&current.id)
                                .is_some_and(|known| known.ordinal > stored.ordinal)
                        })
                        .unwrap_or(jobs.len());
                    jobs.insert(position, job);
                }
            }
            if jobs
                .iter()
                .find(|current| current.id == id)
                .is_none_or(|current| current.machine.stopped || current.machine.paused)
            {
                wake_pending.remove(id);
            }
            backend.known.insert(
                id.to_string(),
                Known {
                    revision: stored.revision,
                    ordinal: stored.ordinal,
                },
            );
        }
        if backend.inflight == 0 {
            backend.removed.clear();
        }
        self.refresh_scopes(scopes);
        Ok(())
    }

    /// Mirrors the legacy save: a scope whose committed hash changed emits one invalidation.
    pub fn refresh_scopes(&mut self, scopes: BTreeSet<ScopeKey>) {
        for scope in scopes {
            let present = self
                .jobs
                .iter()
                .any(|job| job.workspace_id == scope.0 && job.team_id == scope.1);
            let revision = super::identity::scope_revision(&self.jobs, &scope.0, scope.1.as_deref());
            let current = present.then(|| revision.clone());
            if self.committed_revisions.get(&scope) != current.as_ref() {
                self.changes.insert(scope.clone(), revision);
            }
            match current {
                Some(revision) => self.committed_revisions.insert(scope, revision),
                None => self.committed_revisions.remove(&scope),
            };
        }
    }

    /// Replaces the cache with one authoritative committed inventory.
    fn install(&mut self, inventory: Vec<StoredSentinel>, floor: Revision) -> Result<(), String> {
        let jobs = validate(inventory.iter().map(codec::decode).collect())?;
        let Some(backend) = self.backend.sqlite_mut() else {
            return Err(GUARD_FREE_REQUIRED.into());
        };
        let mut known = HashMap::with_capacity(inventory.len());
        for stored in &inventory {
            known.insert(
                stored.record.identity.id.as_str().to_string(),
                Known {
                    revision: stored.revision,
                    ordinal: stored.ordinal,
                },
            );
        }
        for id in backend.known.keys() {
            if !known.contains_key(id) {
                backend.removed.insert(id.clone());
            }
        }
        backend.known = known;
        backend.floor = Some(floor);
        let scopes: BTreeSet<ScopeKey> = self
            .jobs
            .iter()
            .chain(&jobs)
            .map(|job| (job.workspace_id.clone(), job.team_id.clone()))
            .collect();
        self.jobs = jobs;
        self.refresh_scopes(scopes);
        Ok(())
    }

    /// The typed Storage backend over a supplied shared owner; blocking, call off-runtime.
    /// Refuses to start empty when the committed inventory cannot be read or validated.
    pub fn with_storage(storage: &Storage, clock: SharedClock) -> Store {
        let repo = storage.sentinels();
        let loaded = repo.inventory().wait(COMMIT_DEADLINE);
        let mut store = Store {
            jobs: Vec::new(),
            active: HashMap::new(),
            wake_pending: BTreeSet::new(),
            backend: Backend::Sqlite(SqliteBackend {
                repo,
                known: HashMap::new(),
                removed: BTreeSet::new(),
                floor: None,
                inflight: 0,
            }),
            clock,
            unavailable: None,
            admission_closed: false,
            committed_revisions: BTreeMap::new(),
            changes: BTreeMap::new(),
            pending_stop_notices: Vec::new(),
        };
        let installed = loaded
            .map_err(|error| error.to_string())
            .and_then(|read| store.install(read.value, read.revision));
        match installed {
            Ok(()) => {
                store.committed_revisions = scope_revisions(&store.jobs);
                store.changes.clear();
            }
            Err(_) => store.unavailable = Some(LoadFailure::LoadFailed),
        }
        store
    }
}

#[cfg(test)]
impl Store {
    /// Lock-held legacy entry point; the typed backend refuses before staging anything.
    pub fn locked<T: Send + 'static>(
        &mut self,
        stage: impl FnOnce(&mut Store) -> Staged<T>,
    ) -> Result<T, String> {
        if !self.backend.is_json() {
            return Err(GUARD_FREE_REQUIRED.into());
        }
        stage(self).into_ready()
    }
}

/// Resolves an unconfirmed commit by reading every stage through the writer, after all
/// earlier admitted writes settled; only then does admission reopen.
pub fn reconcile(store: &Mutex<Store>) -> Result<bool, String> {
    let repo = {
        let store = store.lock();
        match (&store.backend, store.unavailable) {
            (Backend::Sqlite(backend), Some(LoadFailure::PersistenceUnconfirmed)) => {
                backend.repo.clone()
            }
            _ => return Ok(false),
        }
    };
    let settled = repo
        .settled_inventory()
        .wait(COMMIT_DEADLINE)
        .map_err(|error| format!("sentinel reconciliation failed: {error}"))?;
    let mut store = store.lock();
    if store.unavailable != Some(LoadFailure::PersistenceUnconfirmed) {
        return Ok(false);
    }
    store.install(settled.value, settled.receipt.revision)?;
    store.unavailable = None;
    Ok(true)
}

/// One coherent committed scope plus the live (ephemeral) leases, read with no lock held
/// while Storage serves the snapshot.
pub fn committed_scope(
    store: &Mutex<Store>,
    workspace: &str,
    team: Option<&str>,
) -> Result<(Vec<Sentinel>, BTreeSet<String>), String> {
    let repo = {
        let store = store.lock();
        store.ensure_available()?;
        let leases = store.live_leases();
        match &store.backend {
            Backend::Sqlite(backend) => (backend.repo.clone(), leases),
            #[cfg(test)]
            Backend::Json(_) => {
                let jobs = store
                    .list()
                    .iter()
                    .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
                    .cloned()
                    .collect();
                return Ok((jobs, leases));
            }
        }
    };
    let (repo, leases) = repo;
    let read = repo
        .scope(workspace.to_string(), team.map(str::to_owned))
        .wait(COMMIT_DEADLINE)
        .map_err(|error| {
            format!("blueprint_store_load_failed: committed scope read failed: {error}")
        })?;
    Ok((read.value.iter().map(codec::decode).collect(), leases))
}

pub fn committed_workspace(
    store: &Mutex<Store>,
    workspace: &str,
) -> Result<(Vec<Sentinel>, BTreeSet<String>), String> {
    let repo = {
        let store = store.lock();
        store.ensure_available()?;
        let leases = store.live_leases();
        match &store.backend {
            Backend::Sqlite(backend) => (backend.repo.clone(), leases),
            #[cfg(test)]
            Backend::Json(_) => {
                let jobs = store
                    .list()
                    .iter()
                    .filter(|job| job.workspace_id == workspace)
                    .cloned()
                    .collect();
                return Ok((jobs, leases));
            }
        }
    };
    let (repo, leases) = repo;
    let read = repo
        .workspace(workspace.to_string())
        .wait(COMMIT_DEADLINE)
        .map_err(|error| {
            format!("blueprint_store_load_failed: committed workspace read failed: {error}")
        })?;
    Ok((read.value.iter().map(codec::decode).collect(), leases))
}

/// Why a legacy Sentinel file does not decode into import records.
#[allow(dead_code)] // The all-domain importer consumes this; tests drive it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacySentinelError {
    /// The bytes are not a typed Sentinel inventory.
    Malformed(String),
    /// A well-formed inventory that breaks a stored-inventory invariant.
    Conflict(String),
}

/// Side-effect-free decode of the legacy file into exact import records; it never renames,
/// rewrites or holds the source and starts no work.
#[allow(dead_code)] // The all-domain importer consumes this; tests drive it now.
pub fn legacy_import(raw: &str) -> Result<Vec<SentinelImport>, LegacySentinelError> {
    let jobs: Vec<Sentinel> = serde_json::from_str(raw)
        .map_err(|error| LegacySentinelError::Malformed(error.to_string()))?;
    let jobs = validate(jobs).map_err(LegacySentinelError::Conflict)?;
    jobs.iter()
        .map(codec::import)
        .collect::<Result<_, _>>()
        .map_err(LegacySentinelError::Conflict)
}
