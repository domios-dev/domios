//! Typed refusal diagnostics, the versioned line formats of the capture, import and refusal
//! manifests and of the receipt, and the completion marker committed in `storage_meta`.

use super::compare::ProjectionMismatch;
use super::source_inventory::{
    Captured, EntryKind, Sha256Digest, SourceCapture, SourceFile, SourceRole,
};
use super::{
    DecodeFailure, DecodeFailureKind, IgnoredKeys, ImportDomain, ImportProvenance, ImportReceipt,
};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::cron::{CronDivergenceOrigin, CronJobId, CronRunField, CronRunId};
use crate::repositories::{to_sql_u64, Revision};
use rusqlite::{params, Connection};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

/// Version of every manifest line format and of the committed marker.
pub const IMPORT_MANIFEST_FORMAT: u64 = 1;
const MAX_DETAIL_BYTES: usize = 256;
const MARKER_PREFIX: &str = "legacy_import.";
const MARKER_REVISION: &str = "legacy_import.revision";
const MARKER_FORMAT: &str = "legacy_import.format";
const MARKER_MANIFEST: &str = "legacy_import.manifest_sha256:";

/// What a decode of one captured original found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    Absent,
    Valid,
    Unreadable(String),
    Malformed(String),
    Partial(String),
    Unsupported(String),
    Conflict(String),
}

impl SourceStatus {
    /// Damage a selected rotating backup may stand in for; newer or conflicting facts never are.
    pub fn recoverable(&self) -> bool {
        matches!(
            self,
            SourceStatus::Unreadable(_) | SourceStatus::Malformed(_) | SourceStatus::Partial(_)
        )
    }

    pub fn code(&self) -> &'static str {
        match self {
            SourceStatus::Absent => "absent",
            SourceStatus::Valid => "valid",
            SourceStatus::Unreadable(_) => "unreadable",
            SourceStatus::Malformed(_) => "malformed",
            SourceStatus::Partial(_) => "partial",
            SourceStatus::Unsupported(_) => "unsupported",
            SourceStatus::Conflict(_) => "conflict",
        }
    }

    fn detail(&self) -> Option<&str> {
        match self {
            SourceStatus::Absent | SourceStatus::Valid => None,
            SourceStatus::Unreadable(detail)
            | SourceStatus::Malformed(detail)
            | SourceStatus::Partial(detail)
            | SourceStatus::Unsupported(detail)
            | SourceStatus::Conflict(detail) => Some(detail),
        }
    }
}

impl SourceStatus {
    pub(crate) fn unreadable(kind: EntryKind, detail: &str) -> Self {
        SourceStatus::Unreadable(format!("{}: {detail}", kind.as_str()))
    }
}

impl From<DecodeFailure> for SourceStatus {
    fn from(failure: DecodeFailure) -> Self {
        let detail = bounded(&failure.detail);
        match failure.kind {
            DecodeFailureKind::Malformed => SourceStatus::Malformed(detail),
            DecodeFailureKind::Partial => SourceStatus::Partial(detail),
            DecodeFailureKind::Unsupported => SourceStatus::Unsupported(detail),
            DecodeFailureKind::Conflict => SourceStatus::Conflict(detail),
        }
    }
}

/// A rotating backup an operator may select instead of a damaged current original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCandidate {
    pub relative: String,
    pub role: SourceRole,
    pub status: SourceStatus,
}

/// Why nothing was imported. Every refusal leaves the originals, the backup copies and the
/// committed database exactly as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportRefusal {
    InvalidRequest {
        reason: &'static str,
    },
    /// The source is the importing Storage's own instance root, not an inert copy.
    LiveRoot,
    OverLimit {
        what: &'static str,
        limit: u64,
        observed: u64,
    },
    Unreadable {
        relative: String,
        detail: String,
    },
    SourceChanged {
        relative: String,
    },
    /// An earlier staged copy or manifest differs from what this attempt would write.
    StagingConflict {
        relative: String,
    },
    BackupTampered {
        relative: String,
    },
    /// The current original is absent or damaged while recovery assets exist; an explicit
    /// selection is required and nothing is chosen silently.
    RecoveryRequired {
        domain: ImportDomain,
        current: SourceStatus,
        candidates: Vec<RecoveryCandidate>,
    },
    Decode {
        domain: ImportDomain,
        relative: String,
        status: SourceStatus,
    },
    InvalidSelection {
        domain: ImportDomain,
        reason: &'static str,
    },
    WrongDomain {
        expected: ImportDomain,
        decoded: ImportDomain,
    },
    Bound {
        domain: ImportDomain,
        bound: &'static str,
        limit: u64,
        observed: u64,
    },
    Invalid {
        domain: ImportDomain,
        detail: String,
    },
    DomainOccupied {
        domain: ImportDomain,
    },
    Fenced {
        domain: ImportDomain,
        detail: String,
    },
    ProjectionMismatch {
        domain: ImportDomain,
        mismatches: Vec<ProjectionMismatch>,
        elided: u64,
    },
    AlreadyImported {
        marker: ImportMarker,
    },
}

impl ImportRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            ImportRefusal::InvalidRequest { .. } => "invalid-request",
            ImportRefusal::LiveRoot => "live-root",
            ImportRefusal::OverLimit { .. } => "over-limit",
            ImportRefusal::Unreadable { .. } => "unreadable",
            ImportRefusal::SourceChanged { .. } => "source-changed",
            ImportRefusal::StagingConflict { .. } => "staging-conflict",
            ImportRefusal::BackupTampered { .. } => "backup-tampered",
            ImportRefusal::RecoveryRequired { .. } => "recovery-required",
            ImportRefusal::Decode { .. } => "decode",
            ImportRefusal::InvalidSelection { .. } => "invalid-selection",
            ImportRefusal::WrongDomain { .. } => "wrong-domain",
            ImportRefusal::Bound { .. } => "bound",
            ImportRefusal::Invalid { .. } => "invalid",
            ImportRefusal::DomainOccupied { .. } => "domain-occupied",
            ImportRefusal::Fenced { .. } => "fenced",
            ImportRefusal::ProjectionMismatch { .. } => "projection-mismatch",
            ImportRefusal::AlreadyImported { .. } => "already-imported",
        }
    }

    /// Bounded one-line description; data values are never part of it beyond identities.
    pub fn detail(&self) -> String {
        let text = match self {
            ImportRefusal::InvalidRequest { reason } => (*reason).to_string(),
            ImportRefusal::LiveRoot => "the source is this instance's live root".to_string(),
            ImportRefusal::OverLimit {
                what,
                limit,
                observed,
            } => format!("{what}: {observed} exceeds {limit}"),
            ImportRefusal::Unreadable { relative, detail } => format!("{relative}: {detail}"),
            ImportRefusal::SourceChanged { relative }
            | ImportRefusal::StagingConflict { relative }
            | ImportRefusal::BackupTampered { relative } => relative.clone(),
            ImportRefusal::RecoveryRequired {
                domain,
                current,
                candidates,
            } => format!(
                "{}: current is {}; {} rotating backups recorded",
                domain.as_str(),
                current.code(),
                candidates.len()
            ),
            ImportRefusal::Decode {
                domain,
                relative,
                status,
            } => format!(
                "{} {relative}: {} {}",
                domain.as_str(),
                status.code(),
                status.detail().unwrap_or("")
            ),
            ImportRefusal::InvalidSelection { domain, reason } => {
                format!("{}: {reason}", domain.as_str())
            }
            ImportRefusal::WrongDomain { expected, decoded } => {
                format!(
                    "expected {}, decoded {}",
                    expected.as_str(),
                    decoded.as_str()
                )
            }
            ImportRefusal::Bound {
                domain,
                bound,
                limit,
                observed,
            } => format!("{} {bound}: {observed} exceeds {limit}", domain.as_str()),
            ImportRefusal::Invalid { domain, detail }
            | ImportRefusal::Fenced { domain, detail } => {
                format!("{}: {detail}", domain.as_str())
            }
            ImportRefusal::DomainOccupied { domain } => {
                format!("{} already holds rows", domain.as_str())
            }
            ImportRefusal::ProjectionMismatch {
                domain,
                mismatches,
                elided,
            } => format!(
                "{}: {} differences{}",
                domain.as_str(),
                mismatches.len() as u64 + elided,
                mismatches
                    .first()
                    .map(|first| format!(", first {} {}", first.subject, first.field))
                    .unwrap_or_default()
            ),
            ImportRefusal::AlreadyImported { marker } => {
                format!(
                    "committed at {} as {}",
                    marker.revision, marker.manifest_sha256
                )
            }
        };
        bounded(&text)
    }
}

/// Where the refusal manifest of a refused attempt was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedManifest {
    File(PathBuf),
    /// The capture never completed, so no inventory exists to record against.
    NotCaptured,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalReport {
    pub refusal: ImportRefusal,
    pub recorded: RecordedManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    Refused(Box<RefusalReport>),
    Storage(StorageError),
}

impl MigrationError {
    pub fn refusal(&self) -> Option<&ImportRefusal> {
        match self {
            MigrationError::Refused(report) => Some(&report.refusal),
            MigrationError::Storage(_) => None,
        }
    }
}

impl From<StorageError> for MigrationError {
    fn from(error: StorageError) -> Self {
        MigrationError::Storage(error)
    }
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::Refused(report) => write!(
                f,
                "legacy import refused ({}): {}",
                report.refusal.code(),
                report.refusal.detail()
            ),
            MigrationError::Storage(error) => write!(f, "legacy import failed: {error}"),
        }
    }
}

impl std::error::Error for MigrationError {}

pub(crate) fn uncaptured(refusal: ImportRefusal) -> MigrationError {
    refused(refusal, RecordedManifest::NotCaptured)
}

pub(crate) fn refused(refusal: ImportRefusal, recorded: RecordedManifest) -> MigrationError {
    MigrationError::Refused(Box::new(RefusalReport { refusal, recorded }))
}

/// Both original copies of one imported last run, identified by content digest; the
/// imported revision is the completion marker's revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDivergence {
    pub job_id: CronJobId,
    pub run_id: CronRunId,
    pub origin: CronDivergenceOrigin,
    pub imported_differing: Vec<CronRunField>,
    pub history_copy_sha256: Sha256Digest,
    pub last_run_copy_sha256: Sha256Digest,
}

/// The committed completion marker: the import's revision and its manifest digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportMarker {
    pub revision: Revision,
    pub manifest_sha256: Sha256Digest,
    pub format: u64,
}

/// Preflight facts recorded in the import manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    lines: Vec<String>,
    pub(crate) divergences: Vec<ExpectedDivergence>,
}

impl Facts {
    pub(crate) fn bound(&mut self, domain: ImportDomain, name: &str, limit: u64, observed: u64) {
        self.lines.push(format!(
            "bound {} {} {limit} {observed}",
            domain.as_str(),
            token(name)
        ));
    }

    /// How a limit is enforced when the importer cannot observe it as a number.
    pub(crate) fn rule(&mut self, domain: ImportDomain, name: &str, disposition: &str) {
        self.lines.push(format!(
            "rule {} {} {}",
            domain.as_str(),
            token(name),
            token(disposition)
        ));
    }

    /// Keys the decoder read past, by owner; a comma inside a key stays encoded.
    pub(crate) fn ignored(&mut self, domain: ImportDomain, ignored: &IgnoredKeys) {
        let keys: Vec<String> = ignored
            .keys
            .iter()
            .map(|key| token(key).replace(',', "%2C"))
            .collect();
        self.lines.push(format!(
            "rule {} unknown-keys {} {}",
            domain.as_str(),
            token(&ignored.owner),
            keys.join(",")
        ));
    }

    pub(crate) fn count(&mut self, domain: ImportDomain, name: &str, value: u64) {
        self.lines
            .push(format!("count {} {} {value}", domain.as_str(), token(name)));
    }
}

/// Percent-encodes everything outside a small safe set, so a line splits on spaces exactly.
pub(crate) fn token(text: &str) -> String {
    if text.is_empty() {
        return "~".to_string();
    }
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"._-/:+=,@".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Diagnostics only: a detail longer than the bound is cut and says how much was cut.
pub(crate) fn bounded(text: &str) -> String {
    if text.len() <= MAX_DETAIL_BYTES {
        return text.to_string();
    }
    let mut end = MAX_DETAIL_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(+{} bytes)", &text[..end], text.len() - end)
}

fn header(out: &mut String, kind: &str) {
    let _ = writeln!(out, "codeterm-legacy-{kind} {IMPORT_MANIFEST_FORMAT}");
}

pub(crate) fn render_capture(source_root: &Path, files: &[SourceFile]) -> Vec<u8> {
    let mut out = String::new();
    header(&mut out, "capture");
    let _ = writeln!(out, "source-root {}", token(&source_root.to_string_lossy()));
    for file in files {
        let _ = write!(
            out,
            "file {} {} {} ",
            file.domain.as_str(),
            file.role.as_str(),
            token(&file.relative)
        );
        let _ = match &file.captured {
            Captured::Bytes { size, sha256 } => writeln!(out, "{size} {sha256}"),
            Captured::Unreadable { kind, detail } => {
                writeln!(out, "unreadable {} {}", kind.as_str(), token(detail))
            }
        };
    }
    let _ = writeln!(out, "end {}", files.len());
    out.into_bytes()
}

/// What an import manifest or refusal manifest knows about the attempt so far.
pub(crate) struct PlanContext<'a> {
    pub(crate) capture: &'a SourceCapture,
    pub(crate) provenance: &'a [(ImportDomain, ImportProvenance)],
    pub(crate) statuses: &'a [(ImportDomain, String, SourceStatus)],
}

fn render_context(out: &mut String, context: &PlanContext<'_>) {
    let _ = writeln!(out, "capture {}", context.capture.digest());
    for (domain, provenance) in context.provenance {
        let _ = match provenance {
            ImportProvenance::Current(relative) => {
                writeln!(
                    out,
                    "provenance {} current {}",
                    domain.as_str(),
                    token(relative)
                )
            }
            ImportProvenance::Backup(relative) => {
                writeln!(
                    out,
                    "provenance {} backup {}",
                    domain.as_str(),
                    token(relative)
                )
            }
            ImportProvenance::FirstInstall => {
                writeln!(out, "provenance {} first-install", domain.as_str())
            }
            ImportProvenance::AcknowledgedAbsent => {
                writeln!(out, "provenance {} acknowledged-absent", domain.as_str())
            }
        };
    }
    for (domain, relative, status) in context.statuses {
        let _ = write!(
            out,
            "status {} {} {}",
            domain.as_str(),
            token(relative),
            status.code()
        );
        let _ = match status.detail() {
            Some(detail) => writeln!(out, " {}", token(detail)),
            None => writeln!(out),
        };
    }
}

fn origin_text(origin: CronDivergenceOrigin) -> &'static str {
    match origin {
        CronDivergenceOrigin::LegacyImport => "legacy_import",
    }
}

fn fields_text(fields: &[CronRunField]) -> String {
    token(&CronRunField::join(fields))
}

pub(crate) fn render_import(context: &PlanContext<'_>, facts: &Facts) -> Vec<u8> {
    let mut out = String::new();
    header(&mut out, "import");
    render_context(&mut out, context);
    for line in &facts.lines {
        let _ = writeln!(out, "{line}");
    }
    for divergence in &facts.divergences {
        let _ = writeln!(
            out,
            "divergence {} {} {} at-marker {} {} {}",
            token(divergence.job_id.as_str()),
            token(divergence.run_id.as_str()),
            origin_text(divergence.origin),
            fields_text(&divergence.imported_differing),
            divergence.history_copy_sha256,
            divergence.last_run_copy_sha256
        );
    }
    let _ = writeln!(out, "end");
    out.into_bytes()
}

pub(crate) fn render_refusal(context: &PlanContext<'_>, refusal: &ImportRefusal) -> Vec<u8> {
    let mut out = String::new();
    header(&mut out, "refusal");
    render_context(&mut out, context);
    let _ = writeln!(
        out,
        "refusal {} {}",
        refusal.code(),
        token(&refusal.detail())
    );
    match refusal {
        ImportRefusal::RecoveryRequired {
            domain, candidates, ..
        } => {
            for candidate in candidates {
                let _ = writeln!(
                    out,
                    "candidate {} {} {} {}",
                    domain.as_str(),
                    token(&candidate.relative),
                    candidate.role.as_str(),
                    candidate.status.code()
                );
            }
        }
        ImportRefusal::ProjectionMismatch {
            domain, mismatches, ..
        } => {
            for mismatch in mismatches {
                let _ = writeln!(
                    out,
                    "mismatch {} {} {}",
                    domain.as_str(),
                    token(&mismatch.subject),
                    mismatch.field
                );
            }
        }
        _ => {}
    }
    let _ = writeln!(out, "end");
    out.into_bytes()
}

pub(crate) fn render_receipt(receipt: &ImportReceipt) -> Vec<u8> {
    let mut out = String::new();
    header(&mut out, "receipt");
    let _ = writeln!(out, "manifest {}", receipt.manifest_sha256);
    let _ = writeln!(out, "capture {}", receipt.capture_sha256);
    let _ = writeln!(out, "revision {}", receipt.revision);
    for divergence in &receipt.divergences {
        let _ = writeln!(
            out,
            "divergence {} {} {} {} {} {} {}",
            token(divergence.job_id.as_str()),
            token(divergence.run_id.as_str()),
            origin_text(divergence.origin),
            receipt.revision,
            fields_text(&divergence.imported_differing),
            divergence.history_copy_sha256,
            divergence.last_run_copy_sha256
        );
    }
    let _ = match &receipt.archive {
        Ok(()) => writeln!(out, "archive verified"),
        Err(refusal) => writeln!(
            out,
            "archive {} {}",
            refusal.code(),
            token(&refusal.detail())
        ),
    };
    match &receipt.comparison {
        Ok(report) => {
            for domain in &report.domains {
                let _ = writeln!(
                    out,
                    "compare {} entities {} {} items {} {} mismatches {} elided {}",
                    domain.domain.as_str(),
                    domain.expected_entities,
                    domain.committed_entities,
                    domain.expected_items,
                    domain.committed_items,
                    domain.mismatches.len(),
                    domain.elided
                );
                for mismatch in &domain.mismatches {
                    let _ = writeln!(
                        out,
                        "mismatch {} {} {}",
                        domain.domain.as_str(),
                        token(&mismatch.subject),
                        mismatch.field
                    );
                }
            }
        }
        Err(error) => {
            let _ = writeln!(
                out,
                "compare-unavailable {}",
                token(&bounded(&error.to_string()))
            );
        }
    }
    let _ = writeln!(out, "end");
    out.into_bytes()
}

/// `None` only when no import ever committed; any partial or unknown marker is corrupt.
pub(crate) fn read_marker(conn: &Connection) -> Result<Option<ImportMarker>, StorageError> {
    let corrupt = |detail: &str| StorageError::Corrupt {
        detail: format!("legacy import marker {detail}"),
    };
    let rows: Vec<(String, i64)> = {
        let mut statement = conn
            .prepare_cached(
                "SELECT key, value FROM storage_meta WHERE substr(key, 1, 14) = ?1 ORDER BY key",
            )
            .map_err(|e| sqlite_error("read import marker", e))?;
        let rows = statement
            .query_map([MARKER_PREFIX], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| sqlite_error("read import marker", e))?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| sqlite_error("read import marker", e))?
    };
    if rows.is_empty() {
        return Ok(None);
    }
    let (mut revision, mut format, mut manifest) = (None, None, None);
    for (key, value) in rows {
        let value = u64::try_from(value).map_err(|_| corrupt("holds a negative value"))?;
        if key == MARKER_REVISION {
            revision = Some(value);
        } else if key == MARKER_FORMAT {
            format = Some(value);
        } else if let Some(hex) = key.strip_prefix(MARKER_MANIFEST) {
            let digest =
                Sha256Digest::parse_hex(hex).ok_or_else(|| corrupt("digest is malformed"))?;
            if manifest.replace((digest, value)).is_some() {
                return Err(corrupt("repeats its manifest digest"));
            }
        } else {
            return Err(corrupt("has an unknown key"));
        }
    }
    match (revision, format, manifest) {
        (Some(revision), Some(IMPORT_MANIFEST_FORMAT), Some((digest, bound)))
            if revision > 0 && bound == revision =>
        {
            Ok(Some(ImportMarker {
                revision: Revision(revision),
                manifest_sha256: digest,
                format: IMPORT_MANIFEST_FORMAT,
            }))
        }
        (_, Some(format), _) if format != IMPORT_MANIFEST_FORMAT => {
            Err(corrupt("has an unknown format"))
        }
        _ => Err(corrupt("is incomplete")),
    }
}

/// Commits the marker inside the import's own transaction, at the import's revision.
pub(crate) fn write_marker(cx: &mut WriteCx<'_>, digest: Sha256Digest) -> Result<(), StorageError> {
    let revision = to_sql_u64("revision", cx.revision.0)?;
    let format = to_sql_u64("import format", IMPORT_MANIFEST_FORMAT)?;
    for (key, value) in [
        (MARKER_REVISION.to_string(), revision),
        (MARKER_FORMAT.to_string(), format),
        (format!("{MARKER_MANIFEST}{digest}"), revision),
    ] {
        cx.write(
            "INSERT INTO storage_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    Ok(())
}
