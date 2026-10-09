//! One-shot import of the legacy JSON domains into an empty `state.db`: a bounded capture of
//! every original into an immutable backup, typed preflight, then one all-domain transaction
//! that commits the completion marker with the data. The importer only ever holds decoded
//! data; it has no path to schedule, run, rearm or send anything.

mod compare;
mod cron;
mod desk;
mod live_capture;
mod manifest;
mod sentinel;
mod source_inventory;

pub use compare::{ComparisonReport, DomainComparison, ProjectionMismatch};
pub use live_capture::LiveRootImportRequest;
pub use manifest::{
    ExpectedDivergence, ImportMarker, ImportRefusal, MigrationError, RecordedManifest,
    RecoveryCandidate, RefusalReport, SourceStatus, IMPORT_MANIFEST_FORMAT,
};
pub use source_inventory::{
    capture_sources, CaptureLimits, CaptureStep, Captured, EntryKind, Sha256Digest, SourceCapture,
    SourceFile, SourceRole,
};

use crate::error::StorageError;
use crate::owner::{lock, Storage, WriteCx};
use crate::repositories::cron::CronImport;
use crate::repositories::sentinel::{SentinelCommit, SentinelImport};
use crate::repositories::{Completion, Read, Revision};
use manifest::{refused, uncaptured, Facts, PlanContext};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const IMPORT_MANIFEST: &str = "import.manifest";
const IMPORT_RECEIPT: &str = "import.receipt";
const REFUSALS: &str = "refusals";

/// A legacy domain one import must cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ImportDomain {
    Desk,
    Cron,
    Sentinel,
}

impl ImportDomain {
    /// Every domain, in transaction order; an import requires each exactly once.
    pub const ALL: &'static [ImportDomain] = &[
        ImportDomain::Cron,
        ImportDomain::Sentinel,
        ImportDomain::Desk,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ImportDomain::Desk => "desk",
            ImportDomain::Cron => "cron",
            ImportDomain::Sentinel => "blueprint",
        }
    }
}

/// One domain's original state, decoded from its selected backup copy. Each variant is a
/// typed join of the single import transaction.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DomainImport {
    Desk(crate::repositories::desk::DeskSnapshot),
    Cron(CronImport),
    Sentinel(Vec<SentinelImport>),
}

/// What a domain wrote, handed to its own verification before commit.
enum Written {
    Desk,
    Cron,
    Sentinel(SentinelCommit),
}

/// Why a transaction step stopped; a refusal rolls back exactly like a storage failure.
enum Halt {
    Refused(ImportRefusal),
    Storage(StorageError),
}

impl From<StorageError> for Halt {
    fn from(error: StorageError) -> Self {
        Halt::Storage(error)
    }
}

impl From<ImportRefusal> for Halt {
    fn from(refusal: ImportRefusal) -> Self {
        Halt::Refused(refusal)
    }
}

impl DomainImport {
    pub fn domain(&self) -> ImportDomain {
        match self {
            DomainImport::Desk(_) => ImportDomain::Desk,
            DomainImport::Cron(_) => ImportDomain::Cron,
            DomainImport::Sentinel(_) => ImportDomain::Sentinel,
        }
    }

    fn empty(domain: ImportDomain) -> Self {
        match domain {
            ImportDomain::Desk => DomainImport::Desk(desk::empty()),
            ImportDomain::Cron => DomainImport::Cron(CronImport::default()),
            ImportDomain::Sentinel => DomainImport::Sentinel(Vec::new()),
        }
    }

    fn preflight(&self, facts: &mut Facts) -> Result<(), ImportRefusal> {
        match self {
            DomainImport::Desk(snapshot) => desk::preflight(snapshot, facts),
            DomainImport::Cron(import) => cron::preflight(import, facts),
            DomainImport::Sentinel(records) => sentinel::preflight(records, facts),
        }
    }

    fn occupied(&self, conn: &Connection) -> Result<bool, StorageError> {
        match self {
            DomainImport::Desk(_) => desk::occupied(conn),
            DomainImport::Cron(_) => cron::occupied(conn),
            DomainImport::Sentinel(_) => sentinel::occupied(conn),
        }
    }

    fn write(&self, cx: &mut WriteCx<'_>) -> Result<Written, Halt> {
        match self {
            DomainImport::Desk(snapshot) => {
                desk::write(cx, snapshot.clone())?;
                Ok(Written::Desk)
            }
            DomainImport::Cron(import) => {
                cron::write(cx, import)?;
                Ok(Written::Cron)
            }
            DomainImport::Sentinel(records) => sentinel::write(cx, records).map(Written::Sentinel),
        }
    }

    fn verify(&self, cx: &WriteCx<'_>, written: &Written, plan: &ImportPlan) -> Result<(), Halt> {
        match (self, written) {
            (DomainImport::Desk(snapshot), Written::Desk) => {
                desk::verify(cx.conn, snapshot).map_err(Halt::Refused)
            }
            (DomainImport::Cron(import), Written::Cron) => {
                cron::verify(cx.conn, import, &plan.facts.divergences, cx.revision)
            }
            (DomainImport::Sentinel(records), Written::Sentinel(commit)) => {
                sentinel::verify(records, commit).map_err(Halt::Refused)
            }
            _ => Err(Halt::Storage(StorageError::Corrupt {
                detail: "legacy import joined a domain with another domain's rows".to_string(),
            })),
        }
    }
}

/// Which captured original is authority for one domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSelection {
    Current,
    /// An explicitly chosen rotating backup, only for an absent or damaged (recoverable)
    /// current original, which stays captured.
    Backup(String),
    /// Independently chooses desk backups; an absent snapshot with no rotating backup is acknowledged.
    DeskBackups {
        snapshot: Option<String>,
        identities: Option<String>,
    },
    /// Explicitly accepts an absent current original although recovery assets exist.
    AcknowledgeAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeFailureKind {
    Malformed,
    /// Truncated or empty: the bytes end inside a value.
    Partial,
    /// Written by a format this build does not read.
    Unsupported,
    /// Well-formed facts that cannot be stored as given.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFailure {
    pub kind: DecodeFailureKind,
    pub detail: String,
}

impl DecodeFailure {
    pub fn new(kind: DecodeFailureKind, detail: impl Into<String>) -> Self {
        DecodeFailure {
            kind,
            detail: manifest::bounded(&detail.into()),
        }
    }
}

/// Keys of an original that the typed decoder read past; the backup copy keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredKeys {
    /// The owning entity, such as a cron job id; empty for the file itself.
    pub owner: String,
    pub keys: Vec<String>,
}

/// Pure decoding of captured original bytes; it must not read files or construct effects.
pub trait LegacyDecoder {
    fn decode(&self, domain: ImportDomain, bytes: &[u8]) -> Result<DomainImport, DecodeFailure>;

    fn decode_desk_snapshot(
        &self,
        bytes: &[u8],
    ) -> Result<crate::repositories::desk::DeskSnapshot, DecodeFailure> {
        match self.decode(ImportDomain::Desk, bytes)? {
            DomainImport::Desk(snapshot) => Ok(snapshot),
            other => Err(DecodeFailure::new(
                DecodeFailureKind::Conflict,
                format!("desk snapshot decoded as {}", other.domain().as_str()),
            )),
        }
    }

    fn decode_desk_identities(
        &self,
        _bytes: &[u8],
    ) -> Result<Vec<crate::repositories::desk::DeskAgentSessionIdentity>, DecodeFailure> {
        Err(DecodeFailure::new(
            DecodeFailureKind::Unsupported,
            "no desk identity catalogue decoder",
        ))
    }

    /// Keys `decode` accepted without keeping, recorded in the manifest, never refused.
    fn ignored_keys(&self, _domain: ImportDomain, _bytes: &[u8]) -> Vec<IgnoredKeys> {
        Vec::new()
    }
}

/// Where one domain's imported state came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportProvenance {
    Current(String),
    Backup(String),
    FirstInstall,
    AcknowledgedAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    /// An inert copy of an instance root, never the importing Storage's own root.
    pub source_root: PathBuf,
    /// New, or an earlier attempt's backup of the same copy; disjoint from the source.
    pub backup_dir: PathBuf,
    pub limits: CaptureLimits,
    pub selections: BTreeMap<ImportDomain, SourceSelection>,
    /// Failure bound around each Storage completion wait; expiry is unconfirmed.
    pub deadline: Duration,
}

impl ImportRequest {
    pub fn new(source_root: impl Into<PathBuf>, backup_dir: impl Into<PathBuf>) -> Self {
        ImportRequest {
            source_root: source_root.into(),
            backup_dir: backup_dir.into(),
            limits: CaptureLimits::default(),
            selections: BTreeMap::new(),
            deadline: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    Committed(Box<ImportReceipt>),
    /// An earlier import already committed; no source was opened.
    AlreadyCommitted(ImportMarker),
}

/// The committed import: its revision, the digests binding it to the manifest and the
/// capture, and the comparison of committed reads with the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReceipt {
    pub revision: Revision,
    pub manifest_sha256: Sha256Digest,
    pub capture_sha256: Sha256Digest,
    pub divergences: Vec<ExpectedDivergence>,
    pub comparison: Result<ComparisonReport, StorageError>,
    /// The sealed copies and both manifests re-hashed after commit.
    pub archive: Result<(), ImportRefusal>,
    pub recorded: RecordedManifest,
}

/// Decoded, bound-checked domains with a published import manifest, ready for one commit.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    capture: SourceCapture,
    domains: Vec<DomainImport>,
    provenance: Vec<(ImportDomain, ImportProvenance)>,
    statuses: Vec<(ImportDomain, String, SourceStatus)>,
    facts: Facts,
    manifest: Vec<u8>,
    digest: Sha256Digest,
    cost: usize,
}

#[derive(Default)]
struct Draft {
    provenance: Vec<(ImportDomain, ImportProvenance)>,
    statuses: Vec<(ImportDomain, String, SourceStatus)>,
    domains: Vec<DomainImport>,
    facts: Facts,
}

impl Draft {
    fn fill(
        &mut self,
        capture: &SourceCapture,
        decoder: &dyn LegacyDecoder,
        selections: &BTreeMap<ImportDomain, SourceSelection>,
    ) -> Result<(), ImportRefusal> {
        for &domain in ImportDomain::ALL {
            let selection = selections
                .get(&domain)
                .cloned()
                .unwrap_or(SourceSelection::Current);
            let (provenance, import, ignored) = if domain == ImportDomain::Desk {
                desk::select_desk(capture, decoder, &selection, &mut self.statuses)?
            } else {
                let (origin, import, ignored) =
                    select(capture, decoder, domain, &selection, &mut self.statuses)?;
                (vec![(domain, origin)], import, ignored)
            };
            for keys in &ignored {
                self.facts.ignored(domain, keys);
            }
            self.provenance.extend(provenance);
            self.domains.push(import);
        }
        for import in &self.domains {
            import.preflight(&mut self.facts)?;
        }
        sentinel::cross_relations(&self.domains, &mut self.facts);
        Ok(())
    }

    fn refuse(&self, capture: &SourceCapture, refusal: ImportRefusal) -> MigrationError {
        record_refusal(
            &PlanContext {
                capture,
                provenance: &self.provenance,
                statuses: &self.statuses,
            },
            refusal,
        )
    }
}

impl ImportPlan {
    /// Decodes the selected backup copy of every domain, applies the accepted repository
    /// bounds and publishes the import manifest. Nothing is chosen, skipped or trimmed silently.
    pub fn resolve(
        capture: SourceCapture,
        decoder: &dyn LegacyDecoder,
        selections: &BTreeMap<ImportDomain, SourceSelection>,
    ) -> Result<ImportPlan, MigrationError> {
        let mut draft = Draft::default();
        if let Err(refusal) = draft.fill(&capture, decoder, selections) {
            return Err(draft.refuse(&capture, refusal));
        }
        let manifest = manifest::render_import(
            &PlanContext {
                capture: &capture,
                provenance: &draft.provenance,
                statuses: &draft.statuses,
            },
            &draft.facts,
        );
        match source_inventory::publish_immutable(capture.backup_dir(), IMPORT_MANIFEST, &manifest)
        {
            Ok(_) => {}
            Err(MigrationError::Refused(report)) => {
                return Err(draft.refuse(&capture, report.refusal))
            }
            Err(error) => return Err(error),
        }
        let cost = selected_bytes(&capture, &draft.provenance).saturating_add(manifest.len());
        Ok(ImportPlan {
            digest: Sha256Digest::of(&manifest),
            capture,
            domains: draft.domains,
            provenance: draft.provenance,
            statuses: draft.statuses,
            facts: draft.facts,
            manifest,
            cost,
        })
    }

    pub fn capture(&self) -> &SourceCapture {
        &self.capture
    }

    pub fn domains(&self) -> &[DomainImport] {
        &self.domains
    }

    pub fn provenance(&self) -> &[(ImportDomain, ImportProvenance)] {
        &self.provenance
    }

    pub fn statuses(&self) -> &[(ImportDomain, String, SourceStatus)] {
        &self.statuses
    }

    pub fn divergences(&self) -> &[ExpectedDivergence] {
        &self.facts.divergences
    }

    pub fn manifest_text(&self) -> &[u8] {
        &self.manifest
    }

    pub fn manifest_digest(&self) -> Sha256Digest {
        self.digest
    }

    /// Re-hashes every sealed copy, the capture manifest and the import manifest.
    fn verify_archive(&self) -> Result<(), ImportRefusal> {
        self.capture.verify_sealed()?;
        source_inventory::verify_immutable(
            self.capture.backup_dir(),
            IMPORT_MANIFEST,
            self.manifest.len() as u64,
            self.digest,
        )
    }

    fn refuse(&self, refusal: ImportRefusal) -> MigrationError {
        record_refusal(
            &PlanContext {
                capture: &self.capture,
                provenance: &self.provenance,
                statuses: &self.statuses,
            },
            refusal,
        )
    }
}

/// Records a refusal manifest named by its own digest, so a repeated refusal is idempotent.
fn record_refusal(context: &PlanContext<'_>, refusal: ImportRefusal) -> MigrationError {
    let text = manifest::render_refusal(context, &refusal);
    let relative = format!("{REFUSALS}/{}.manifest", Sha256Digest::of(&text));
    let recorded =
        match source_inventory::publish_immutable(context.capture.backup_dir(), &relative, &text) {
            Ok(path) => RecordedManifest::File(path),
            Err(error) => RecordedManifest::Failed(manifest::bounded(&error.to_string())),
        };
    refused(refusal, recorded)
}

fn selected_bytes(
    capture: &SourceCapture,
    provenance: &[(ImportDomain, ImportProvenance)],
) -> usize {
    provenance
        .iter()
        .filter_map(|(domain, origin)| match origin {
            ImportProvenance::Current(relative) | ImportProvenance::Backup(relative) => capture
                .files()
                .iter()
                .find(|file| file.domain == *domain && file.relative == *relative),
            ImportProvenance::FirstInstall | ImportProvenance::AcknowledgedAbsent => None,
        })
        .map(|file| match file.captured {
            Captured::Bytes { size, .. } => usize::try_from(size).unwrap_or(usize::MAX),
            Captured::Unreadable { .. } => 0,
        })
        .fold(0, usize::saturating_add)
}

type Statuses = Vec<(ImportDomain, String, SourceStatus)>;
type Decoded = (DomainImport, Vec<IgnoredKeys>);
type Selected = (ImportProvenance, DomainImport, Vec<IgnoredKeys>);

fn select(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    domain: ImportDomain,
    selection: &SourceSelection,
    statuses: &mut Statuses,
) -> Result<Selected, ImportRefusal> {
    let current = capture.file(domain, SourceRole::Current);
    match selection {
        SourceSelection::Current => {
            let Some(file) = current else {
                if !capture.files().iter().any(|file| file.domain == domain) {
                    let empty = DomainImport::empty(domain);
                    return Ok((ImportProvenance::FirstInstall, empty, Vec::new()));
                }
                return Err(ImportRefusal::RecoveryRequired {
                    domain,
                    current: SourceStatus::Absent,
                    candidates: candidates(capture, decoder, domain, statuses)?,
                });
            };
            match decode(capture, decoder, domain, file, statuses)? {
                Ok((import, ignored)) => Ok((
                    ImportProvenance::Current(file.relative.clone()),
                    import,
                    ignored,
                )),
                Err(status) if status.recoverable() => Err(ImportRefusal::RecoveryRequired {
                    domain,
                    current: status,
                    candidates: candidates(capture, decoder, domain, statuses)?,
                }),
                Err(status) => Err(ImportRefusal::Decode {
                    domain,
                    relative: file.relative.clone(),
                    status,
                }),
            }
        }
        SourceSelection::Backup(relative) => {
            let file = capture
                .files()
                .iter()
                .find(|file| {
                    file.domain == domain
                        && file.role == SourceRole::RotatingBackup
                        && file.relative == *relative
                })
                .ok_or(ImportRefusal::InvalidSelection {
                    domain,
                    reason: "names no rotating backup of this domain",
                })?;
            if let Some(current) = current {
                let damaged = match decode(capture, decoder, domain, current, statuses)? {
                    Ok(_) => false,
                    Err(status) => status.recoverable(),
                };
                if !damaged {
                    return Err(ImportRefusal::InvalidSelection {
                        domain,
                        reason: "a backup stands in only for an absent or damaged original",
                    });
                }
            }
            match decode(capture, decoder, domain, file, statuses)? {
                Ok((import, ignored)) => Ok((
                    ImportProvenance::Backup(file.relative.clone()),
                    import,
                    ignored,
                )),
                Err(status) => Err(ImportRefusal::Decode {
                    domain,
                    relative: file.relative.clone(),
                    status,
                }),
            }
        }
        SourceSelection::AcknowledgeAbsent => {
            if current.is_some() {
                return Err(ImportRefusal::InvalidSelection {
                    domain,
                    reason: "the current original exists",
                });
            }
            Ok((
                ImportProvenance::AcknowledgedAbsent,
                DomainImport::empty(domain),
                Vec::new(),
            ))
        }
        SourceSelection::DeskBackups { .. } => Err(ImportRefusal::InvalidSelection {
            domain,
            reason: "desk backup selections apply only to the desk domain",
        }),
    }
}

/// Decodes one verified backup copy; the inner error is the source's status, the outer one
/// a refusal that no selection can resolve.
fn decode(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    domain: ImportDomain,
    file: &SourceFile,
    statuses: &mut Statuses,
) -> Result<Result<Decoded, SourceStatus>, ImportRefusal> {
    let outcome = match &file.captured {
        Captured::Unreadable { kind, detail } => Err(SourceStatus::unreadable(*kind, detail)),
        Captured::Bytes { .. } => {
            let bytes = capture.read(file)?;
            match decoder.decode(domain, &bytes) {
                Ok(import) if import.domain() == domain => {
                    Ok((import, decoder.ignored_keys(domain, &bytes)))
                }
                Ok(other) => {
                    return Err(ImportRefusal::WrongDomain {
                        expected: domain,
                        decoded: other.domain(),
                    })
                }
                Err(failure) => Err(SourceStatus::from(failure)),
            }
        }
    };
    let status = match &outcome {
        Ok(_) => SourceStatus::Valid,
        Err(status) => status.clone(),
    };
    statuses.push((domain, file.relative.clone(), status));
    Ok(outcome)
}

fn candidates(
    capture: &SourceCapture,
    decoder: &dyn LegacyDecoder,
    domain: ImportDomain,
    statuses: &mut Statuses,
) -> Result<Vec<RecoveryCandidate>, ImportRefusal> {
    let mut found = Vec::new();
    for file in capture.files() {
        if file.domain != domain || file.role != SourceRole::RotatingBackup {
            continue;
        }
        let status = match decode(capture, decoder, domain, file, statuses)? {
            Ok(_) => SourceStatus::Valid,
            Err(status) => status,
        };
        found.push(RecoveryCandidate {
            relative: file.relative.clone(),
            role: file.role,
            status,
        });
    }
    Ok(found)
}

/// Test-only interference applied between the domain writes and their verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportTamper {
    /// Renames the first imported cron definition after it was written.
    CronDefinition,
    /// Alters the generation in the Sentinel aggregates read back after their write.
    SentinelReadBack,
    /// Attempts one child insert with a missing domain-owned foreign key target.
    ForeignKey(ImportDomain),
}

#[derive(Clone, Copy)]
enum Tamper {
    Nothing,
    #[cfg(feature = "test-support")]
    Apply(ImportTamper),
}

impl Tamper {
    #[cfg_attr(not(feature = "test-support"), allow(unused_variables))]
    fn apply(self, cx: &mut WriteCx<'_>, written: &mut [Written]) -> Result<(), StorageError> {
        match self {
            Tamper::Nothing => Ok(()),
            #[cfg(feature = "test-support")]
            Tamper::Apply(ImportTamper::CronDefinition) => {
                cx.write(
                    "UPDATE cron_jobs SET name = name || ' (tampered)' WHERE ordinal = 0",
                    [],
                )?;
                Ok(())
            }
            #[cfg(feature = "test-support")]
            Tamper::Apply(ImportTamper::SentinelReadBack) => {
                for item in written.iter_mut() {
                    if let Written::Sentinel(commit) = item {
                        if let Some(first) = commit.sentinels.first_mut() {
                            first.record.runtime.generation += 1;
                        }
                    }
                }
                Ok(())
            }
            #[cfg(feature = "test-support")]
            Tamper::Apply(ImportTamper::ForeignKey(domain)) => {
                let statement = match domain {
                    ImportDomain::Desk => {
                        "INSERT INTO desk_tabs (id, workspace_id, ordinal, title, sort_order, revision) \
                         VALUES ('fk-missing', 'fk-missing', 0, 'fk', 0, 1)"
                    }
                    ImportDomain::Cron => {
                        "INSERT INTO cron_runs (job_id, run_id, started_at, status) \
                         VALUES ('fk-missing', 'fk-missing', 0, 'success')"
                    }
                    ImportDomain::Sentinel => {
                        "INSERT INTO blueprint_definitions (blueprint_id, version, provenance, probe, question, interval_ms, max_transitions, max_actions, created_revision) \
                         VALUES ('fk-missing', 1, 'imported', 'probe', 'question', 1, 1, 1, 1)"
                    }
                };
                cx.write(statement, [])?;
                Ok(())
            }
        }
    }
}

/// Runs inside one savepoint of the writer's IMMEDIATE transaction with SQL and in-memory
/// data only; any error rolls every domain back and the marker never commits alone.
fn transaction(cx: &mut WriteCx<'_>, plan: &ImportPlan, tamper: Tamper) -> Result<(), Halt> {
    if let Some(marker) = manifest::read_marker(cx.conn)? {
        return Err(Halt::Refused(ImportRefusal::AlreadyImported { marker }));
    }
    for import in &plan.domains {
        if import.occupied(cx.conn)? {
            return Err(Halt::Refused(ImportRefusal::DomainOccupied {
                domain: import.domain(),
            }));
        }
    }
    let mut written = Vec::with_capacity(plan.domains.len());
    for import in &plan.domains {
        written.push(import.write(cx)?);
    }
    tamper.apply(cx, &mut written)?;
    for (import, written) in plan.domains.iter().zip(&written) {
        import.verify(cx, written, plan)?;
    }
    manifest::write_marker(cx, plan.digest)?;
    Ok(())
}

impl Storage {
    /// The committed completion marker; `None` only while no import ever committed.
    pub fn legacy_import_marker(&self) -> Completion<Read<Option<ImportMarker>>> {
        self.submit_read(|cx| manifest::read_marker(cx.conn))
    }

    /// Blocking, off the async runtime. A committed marker answers without opening any
    /// source; otherwise capture, resolve and commit one all-domain transaction.
    pub fn import_legacy_root(
        &self,
        request: &ImportRequest,
        decoder: &dyn LegacyDecoder,
    ) -> Result<ImportOutcome, MigrationError> {
        if request.deadline.is_zero() {
            return Err(uncaptured(ImportRefusal::InvalidRequest {
                reason: "the deadline must be positive",
            }));
        }
        if let Some(marker) = self.legacy_import_marker().wait(request.deadline)?.value {
            return Ok(ImportOutcome::AlreadyCommitted(marker));
        }
        let root = self.instance_root(request.deadline)?;
        source_inventory::refuse_live_root(&request.source_root, &root).map_err(uncaptured)?;
        let capture = capture_sources(&request.source_root, &request.backup_dir, request.limits)?;
        let plan = ImportPlan::resolve(capture, decoder, &request.selections)?;
        let receipt = self.commit_legacy_import(plan, request.deadline)?;
        Ok(ImportOutcome::Committed(Box::new(receipt)))
    }

    /// Blocking: one transaction writes every domain, gates the commit on typed projections
    /// and commits the marker; committed reads are then compared again for the receipt.
    pub fn commit_legacy_import(
        &self,
        plan: ImportPlan,
        deadline: Duration,
    ) -> Result<ImportReceipt, MigrationError> {
        self.commit_plan(plan, deadline, Tamper::Nothing, None)
    }

    #[cfg(feature = "test-support")]
    pub fn commit_legacy_import_tampered(
        &self,
        plan: ImportPlan,
        deadline: Duration,
        tamper: ImportTamper,
    ) -> Result<ImportReceipt, MigrationError> {
        self.commit_plan(plan, deadline, Tamper::Apply(tamper), None)
    }

    /// Runs `after_submit` once the verified plan is admitted and before its outcome is
    /// awaited, so a test can act while a held writer keeps the import queued.
    #[cfg(feature = "test-support")]
    pub fn commit_legacy_import_after_submit(
        &self,
        plan: ImportPlan,
        deadline: Duration,
        after_submit: Box<dyn FnOnce()>,
    ) -> Result<ImportReceipt, MigrationError> {
        self.commit_plan(plan, deadline, Tamper::Nothing, Some(after_submit))
    }

    fn instance_root(&self, deadline: Duration) -> Result<PathBuf, MigrationError> {
        self.submit_read(|cx| Ok(cx.layout.dir.parent().map(Path::to_path_buf)))
            .wait(deadline)?
            .value
            .ok_or_else(|| {
                MigrationError::Storage(StorageError::Corrupt {
                    detail: "storage layout has no instance root".to_string(),
                })
            })
    }

    /// Every filesystem check runs here on the calling thread, immediately before submit;
    /// the writer transaction touches only SQL and the already-verified plan.
    fn commit_plan(
        &self,
        plan: ImportPlan,
        deadline: Duration,
        tamper: Tamper,
        after_submit: Option<Box<dyn FnOnce()>>,
    ) -> Result<ImportReceipt, MigrationError> {
        let root = self.instance_root(deadline)?;
        let verified = source_inventory::refuse_live_root(plan.capture.source_root(), &root)
            .and_then(|()| plan.verify_archive());
        if let Err(refusal) = verified {
            return Err(plan.refuse(refusal));
        }
        let plan = Arc::new(plan);
        let slot: Arc<Mutex<Option<ImportRefusal>>> = Arc::default();
        let (job, refusal) = (plan.clone(), slot.clone());
        let submitted = self.submit_mutation(plan.cost, move |cx| {
            transaction(cx, &job, tamper).map_err(|halt| match halt {
                Halt::Storage(error) => error,
                Halt::Refused(found) => {
                    *lock(&refusal) = Some(found);
                    StorageError::invalid("legacy import", "refused before commit")
                }
            })
        });
        if let Some(after_submit) = after_submit {
            after_submit();
        }
        let outcome = submitted.wait(deadline);
        let committed = match outcome {
            Ok(committed) => committed,
            Err(error) => {
                let found = lock(&slot).take();
                return Err(match found {
                    Some(found) => plan.refuse(found),
                    None => MigrationError::Storage(error),
                });
            }
        };
        let revision = committed.receipt.revision;
        let mut receipt = ImportReceipt {
            revision,
            manifest_sha256: plan.digest,
            capture_sha256: plan.capture.digest(),
            divergences: plan.facts.divergences.clone(),
            comparison: compare::verify_committed(self, &plan, revision, deadline),
            archive: plan.verify_archive(),
            recorded: RecordedManifest::NotCaptured,
        };
        let text = manifest::render_receipt(&receipt);
        receipt.recorded = match source_inventory::publish_immutable(
            plan.capture.backup_dir(),
            IMPORT_RECEIPT,
            &text,
        ) {
            Ok(path) => RecordedManifest::File(path),
            Err(error) => RecordedManifest::Failed(manifest::bounded(&error.to_string())),
        };
        Ok(receipt)
    }
}
