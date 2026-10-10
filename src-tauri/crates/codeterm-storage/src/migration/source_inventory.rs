//! Read-only bounded capture of every legacy original into an immutable backup copy.
//! Source paths are only ever opened for reading; nothing here renames, rewrites or deletes.

mod desk;
mod digest;
use digest::Sha256;
pub use digest::Sha256Digest;

use super::manifest::{self, bounded, uncaptured, ImportRefusal, MigrationError};
use super::ImportDomain;
use crate::assets::{
    self, canonical_dir, create_owner_file, ensure_owned_dir, publish_no_replace, sync_dir,
    unique_token,
};
use crate::error::{io_error, StorageError};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 64 * 1024;
const ORIGINALS: &str = "originals";
const STAGING: &str = "tmp";
pub(crate) const CAPTURE_MANIFEST: &str = "capture.manifest";
const LIVE_CAPTURE_EPOCH: &str = "0000000000000001";

/// Count and byte bounds of one capture; exceeding any of them refuses the whole capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_scan_entries: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        CaptureLimits {
            max_files: 128,
            max_file_bytes: 64 << 20,
            max_total_bytes: 256 << 20,
            max_scan_entries: 16_384,
        }
    }
}

impl CaptureLimits {
    pub(crate) fn validate(&self) -> Result<(), ImportRefusal> {
        if self.max_files == 0
            || self.max_file_bytes == 0
            || self.max_total_bytes == 0
            || self.max_scan_entries == 0
        {
            return Err(ImportRefusal::InvalidRequest {
                reason: "capture limits must be positive",
            });
        }
        Ok(())
    }
}

/// Why a legacy file exists; only `Current`, or an explicitly selected rotating backup,
/// can become import authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceRole {
    Current,
    RotatingBackup,
    NamedSnapshot,
    Quarantined,
    InterruptedWrite,
    Unrecognized,
}

impl SourceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceRole::Current => "current",
            SourceRole::RotatingBackup => "rotating-backup",
            SourceRole::NamedSnapshot => "named-snapshot",
            SourceRole::Quarantined => "quarantined",
            SourceRole::InterruptedWrite => "interrupted-write",
            SourceRole::Unrecognized => "unrecognized",
        }
    }
}

/// What a source entry is; links and reparse points are recorded, never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Regular,
    Directory,
    Symlink,
    Junction,
    Reparse,
    Other,
    /// Its metadata could not be read.
    Unstatable,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Regular => "regular",
            EntryKind::Directory => "directory",
            EntryKind::Symlink => "symlink",
            EntryKind::Junction => "junction",
            EntryKind::Reparse => "reparse",
            EntryKind::Other => "other",
            EntryKind::Unstatable => "unstatable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    Bytes {
        size: u64,
        sha256: Sha256Digest,
    },
    /// Enumerated but not copyable: a link, a non-file or an unreadable file stays in place.
    Unreadable {
        kind: EntryKind,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub domain: ImportDomain,
    pub role: SourceRole,
    /// Forward-slash path relative to the source root.
    pub relative: String,
    pub captured: Captured,
}

/// A completed capture: every enumerated original and its immutable backup copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCapture {
    source_root: PathBuf,
    backup_dir: PathBuf,
    files: Vec<SourceFile>,
    digest: Sha256Digest,
    manifest_bytes: u64,
}

impl SourceCapture {
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Digest of the published `capture.manifest`.
    pub fn digest(&self) -> Sha256Digest {
        self.digest
    }

    pub fn original_path(&self, file: &SourceFile) -> PathBuf {
        resolve(&self.backup_dir.join(ORIGINALS), &file.relative)
    }

    pub(crate) fn file(&self, domain: ImportDomain, role: SourceRole) -> Option<&SourceFile> {
        self.files
            .iter()
            .find(|file| file.domain == domain && file.role == role)
    }

    /// Reads one backup copy, refusing unless it still has the captured size and hash.
    pub(crate) fn read(&self, file: &SourceFile) -> Result<Vec<u8>, ImportRefusal> {
        let Captured::Bytes { size, sha256 } = &file.captured else {
            return Err(ImportRefusal::Unreadable {
                relative: file.relative.clone(),
                detail: "was not captured".to_string(),
            });
        };
        let tampered = || ImportRefusal::BackupTampered {
            relative: file.relative.clone(),
        };
        let handle = File::open(self.original_path(file)).map_err(|_| tampered())?;
        let mut bytes = Vec::new();
        handle
            .take(size.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| tampered())?;
        if bytes.len() as u64 != *size || Sha256Digest::of(&bytes) != *sha256 {
            return Err(tampered());
        }
        Ok(bytes)
    }

    /// Re-hashes every backup copy and the capture manifest against the capture.
    pub(crate) fn verify_sealed(&self) -> Result<(), ImportRefusal> {
        verify_immutable(
            &self.backup_dir,
            CAPTURE_MANIFEST,
            self.manifest_bytes,
            self.digest,
        )?;
        for file in &self.files {
            let Captured::Bytes { size, sha256 } = &file.captured else {
                continue;
            };
            let path = self.original_path(file);
            let readonly =
                fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.permissions().readonly());
            let same =
                readonly && hash_path(&path, *size).is_ok_and(|hashed| hashed == (*size, *sha256));
            if !same {
                return Err(ImportRefusal::BackupTampered {
                    relative: file.relative.clone(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn from_live_archive(
        backup_dir: &Path,
        files: Vec<SourceFile>,
        manifest: &[u8],
    ) -> Result<SourceCapture, MigrationError> {
        for file in &files {
            let Captured::Bytes { size, sha256 } = &file.captured else {
                continue;
            };
            let path = resolve(&backup_dir.join(ORIGINALS), &file.relative);
            if hash_path(&path, *size).ok() != Some((*size, *sha256)) {
                return Err(uncaptured(ImportRefusal::BackupTampered {
                    relative: file.relative.clone(),
                }));
            }
            set_readonly(&path)?;
        }
        set_readonly(&backup_dir.join(CAPTURE_MANIFEST))?;
        let capture = SourceCapture {
            source_root: canonical_dir(&backup_dir.join(ORIGINALS))?,
            backup_dir: backup_dir.to_path_buf(),
            files,
            digest: Sha256Digest::of(manifest),
            manifest_bytes: manifest.len() as u64,
        };
        capture.verify_sealed().map_err(uncaptured)?;
        Ok(capture)
    }
}

/// Copies every legacy original of every domain into `backup_dir`. A restart over a partial
/// or complete earlier capture reuses identical copies and refuses any that differ.
pub fn capture_sources(
    source_root: &Path,
    backup_dir: &Path,
    limits: CaptureLimits,
) -> Result<SourceCapture, MigrationError> {
    capture(source_root, backup_dir, limits, &mut |_: &str| true, false)
}

pub(crate) fn capture_live_sources(
    source_root: &Path,
    limits: CaptureLimits,
) -> Result<SourceCapture, MigrationError> {
    let source = canonical_dir(source_root).map_err(|error| {
        uncaptured(ImportRefusal::Unreadable {
            relative: String::new(),
            detail: bounded(&error.to_string()),
        })
    })?;
    let backup = live_backup_dir(&source)?;
    capture(&source, &backup, limits, &mut |_: &str| true, true)
}

/// What a test-support capture observer wants after one original was staged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStep {
    Continue,
    /// Stops as a crash would: staged copies stay and no capture manifest is published.
    Interrupt,
}

#[cfg(feature = "test-support")]
impl SourceCapture {
    /// `capture_sources` with an observer called after each staged original.
    pub fn capture_observed(
        source_root: &Path,
        backup_dir: &Path,
        limits: CaptureLimits,
        observer: &mut dyn FnMut(&str) -> CaptureStep,
    ) -> Result<SourceCapture, MigrationError> {
        capture(
            source_root,
            backup_dir,
            limits,
            &mut |relative: &str| observer(relative) == CaptureStep::Continue,
            false,
        )
    }

    pub(crate) fn capture_live_observed(
        source_root: &Path,
        limits: CaptureLimits,
        observer: &mut dyn FnMut(&str) -> CaptureStep,
    ) -> Result<SourceCapture, MigrationError> {
        let source = canonical_dir(source_root).map_err(|error| {
            uncaptured(ImportRefusal::Unreadable {
                relative: String::new(),
                detail: bounded(&error.to_string()),
            })
        })?;
        let backup = live_backup_dir(&source)?;
        capture(
            &source,
            &backup,
            limits,
            &mut |relative: &str| observer(relative) == CaptureStep::Continue,
            true,
        )
    }
}

fn capture(
    source_root: &Path,
    backup_dir: &Path,
    limits: CaptureLimits,
    observer: &mut dyn FnMut(&str) -> bool,
    live_root: bool,
) -> Result<SourceCapture, MigrationError> {
    limits.validate().map_err(uncaptured)?;
    let source = canonical_dir(source_root).map_err(|error| {
        uncaptured(ImportRefusal::Unreadable {
            relative: String::new(),
            detail: bounded(&error.to_string()),
        })
    })?;
    let backup = if live_root {
        live_backup_dir(&source)?
    } else {
        prepare_backup(&source, backup_dir)?
    };
    let entries = scan(&source, &limits).map_err(uncaptured)?;
    let mut total = 0;
    let mut files = Vec::with_capacity(entries.len());
    for entry in &entries {
        let captured = stage(&source, &backup, entry, &limits, &mut total)?;
        files.push(SourceFile {
            domain: entry.domain,
            role: entry.role,
            relative: entry.relative.clone(),
            captured,
        });
        if !observer(&entry.relative) {
            return Err(MigrationError::Storage(StorageError::Injected {
                point: "capture",
            }));
        }
    }
    confirm_unchanged(&source, &limits, &entries, &files).map_err(uncaptured)?;
    let text = manifest::render_capture(&source, &files);
    publish_immutable(&backup, CAPTURE_MANIFEST, &text)?;
    Ok(SourceCapture {
        source_root: if live_root {
            canonical_dir(&backup.join(ORIGINALS))?
        } else {
            source
        },
        backup_dir: backup,
        files,
        digest: Sha256Digest::of(&text),
        manifest_bytes: text.len() as u64,
    })
}

pub(crate) fn live_backup_dir(source: &Path) -> Result<PathBuf, MigrationError> {
    let storage = source.join("storage");
    ensure_owned_dir(&storage)?;
    let archives = storage.join("legacy-originals");
    ensure_owned_dir(&archives)?;
    let backup = archives.join(LIVE_CAPTURE_EPOCH);
    ensure_owned_dir(&backup)?;
    ensure_owned_dir(&backup.join(ORIGINALS))?;
    ensure_owned_dir(&backup.join(STAGING))?;
    sync_dir(&storage)?;
    sync_dir(&archives)?;
    sync_dir(&backup)?;
    canonical_dir(&backup).map_err(MigrationError::from)
}

/// Only an inert copy is imported: the importing Storage's own root is never a source.
pub(crate) fn refuse_live_root(
    source_root: &Path,
    storage_root: &Path,
) -> Result<(), ImportRefusal> {
    let source = canonical_dir(source_root).map_err(|error| ImportRefusal::Unreadable {
        relative: String::new(),
        detail: bounded(&error.to_string()),
    })?;
    let storage = canonical_dir(storage_root).unwrap_or_else(|_| storage_root.to_path_buf());
    if folded(&source) == folded(&storage) {
        return Err(ImportRefusal::LiveRoot);
    }
    Ok(())
}

fn folded(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

fn prepare_backup(source: &Path, backup_dir: &Path) -> Result<PathBuf, MigrationError> {
    let invalid = |reason| uncaptured(ImportRefusal::InvalidRequest { reason });
    let name = backup_dir
        .file_name()
        .ok_or_else(|| invalid("the backup directory needs a name"))?;
    let parent = backup_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let backup = canonical_dir(parent)?.join(name);
    let (folded_source, folded_backup) = (folded(source), folded(&backup));
    if folded_source.starts_with(&folded_backup) || folded_backup.starts_with(&folded_source) {
        return Err(invalid(
            "the backup and source directories must be disjoint",
        ));
    }
    ensure_owned_dir(&backup)?;
    ensure_owned_dir(&backup.join(ORIGINALS))?;
    ensure_owned_dir(&backup.join(STAGING))?;
    Ok(backup)
}

fn current_name(domain: ImportDomain) -> &'static str {
    match domain {
        ImportDomain::Desk => "sessions/last.json",
        ImportDomain::Cron => "crons.json",
        ImportDomain::Sentinel => "sentinels.json",
    }
}

/// Directories holding one domain's originals, relative to the source root.
fn scan_dirs(domain: ImportDomain) -> Vec<String> {
    match domain {
        ImportDomain::Desk => desk::scan_dirs(),
        ImportDomain::Cron | ImportDomain::Sentinel => vec![
            String::new(),
            format!("state-backups/{}", current_name(domain)),
        ],
    }
}

fn classify(domain: ImportDomain, dir: &str, name: &str) -> Option<SourceRole> {
    if domain == ImportDomain::Desk {
        return desk::classify(dir, name);
    }
    let file = current_name(domain);
    if !dir.is_empty() {
        return Some(if name.ends_with(".bak") {
            SourceRole::RotatingBackup
        } else if name.contains(".tmp.") {
            SourceRole::InterruptedWrite
        } else {
            SourceRole::Unrecognized
        });
    }
    if name == file {
        return Some(SourceRole::Current);
    }
    let rest = name.strip_prefix(file)?.strip_prefix('.')?;
    Some(if rest.starts_with("corrupt-") {
        SourceRole::Quarantined
    } else if rest.starts_with("tmp.") {
        SourceRole::InterruptedWrite
    } else {
        SourceRole::Unrecognized
    })
}

pub(crate) fn valid_live_inventory_path(
    domain: ImportDomain,
    role: SourceRole,
    relative: &str,
) -> bool {
    if domain == ImportDomain::Desk {
        return desk::valid_inventory_path(role, relative);
    }
    let mut parts = relative.split('/');
    let first = parts.next().unwrap_or_default();
    if first.is_empty() || first == "." || first == ".." || (cfg!(windows) && first.contains('\\'))
    {
        return false;
    }
    let (dir, name) = match (first, parts.next(), parts.next(), parts.next()) {
        (name, None, None, None) => ("", name),
        ("state-backups", Some(file), Some(name), None)
            if file == current_name(domain)
                && !name.is_empty()
                && name != "."
                && name != ".."
                && (!cfg!(windows) || !name.contains('\\')) =>
        {
            ("state-backups", name)
        }
        _ => return false,
    };
    if cfg!(windows) && name.contains(':') {
        return false;
    }
    classify(domain, dir, name) == Some(role)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    domain: ImportDomain,
    role: SourceRole,
    relative: String,
}

struct Listed {
    name: String,
    exact: bool,
}

fn resolve(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn join_relative(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn scan(root: &Path, limits: &CaptureLimits) -> Result<Vec<Entry>, ImportRefusal> {
    let mut listed: BTreeMap<String, Vec<Listed>> = BTreeMap::new();
    let mut entries = Vec::new();
    for &domain in ImportDomain::ALL {
        for dir in scan_dirs(domain) {
            if !listed.contains_key(&dir) {
                let names = list_dir(root, &dir, limits)?;
                listed.insert(dir.clone(), names);
            }
            for found in &listed[&dir] {
                let Some(role) = classify(domain, &dir, &found.name) else {
                    continue;
                };
                let relative = join_relative(&dir, &found.name);
                if !found.exact {
                    return Err(ImportRefusal::Unreadable {
                        relative: bounded(&relative),
                        detail: "the name is not valid UTF-8".to_string(),
                    });
                }
                entries.push(Entry {
                    domain,
                    role,
                    relative,
                });
            }
        }
    }
    entries.sort_by(|a, b| (a.domain, a.role, &a.relative).cmp(&(b.domain, b.role, &b.relative)));
    if entries.len() > limits.max_files {
        return Err(ImportRefusal::OverLimit {
            what: "source files",
            limit: limits.max_files as u64,
            observed: entries.len() as u64,
        });
    }
    Ok(entries)
}

fn list_dir(
    root: &Path,
    relative: &str,
    limits: &CaptureLimits,
) -> Result<Vec<Listed>, ImportRefusal> {
    let dir = resolve(root, relative);
    let unreadable = |detail: String| ImportRefusal::Unreadable {
        relative: relative.to_string(),
        detail: bounded(&detail),
    };
    match fs::symlink_metadata(&dir) {
        Err(error) if error.kind() == ErrorKind::NotFound && !relative.is_empty() => {
            return Ok(Vec::new())
        }
        Err(error) => return Err(unreadable(error.to_string())),
        Ok(meta) if !meta.is_dir() || is_link(&meta) => {
            return Err(unreadable("not a real directory".to_string()))
        }
        Ok(_) => {}
    }
    let reader = fs::read_dir(&dir).map_err(|error| unreadable(error.to_string()))?;
    let mut names = Vec::new();
    for entry in reader {
        if names.len() >= limits.max_scan_entries {
            return Err(ImportRefusal::OverLimit {
                what: "directory entries",
                limit: limits.max_scan_entries as u64,
                observed: names.len() as u64 + 1,
            });
        }
        let raw = entry
            .map_err(|error| unreadable(error.to_string()))?
            .file_name();
        names.push(match raw.to_str() {
            Some(name) => Listed {
                name: name.to_string(),
                exact: true,
            },
            None => Listed {
                name: raw.to_string_lossy().into_owned(),
                exact: false,
            },
        });
    }
    names.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(names)
}

struct Budget {
    declared: u64,
    file_limit: u64,
    total_limit: u64,
    total_before: u64,
}

enum Pumped {
    Done(u64, Sha256Digest),
    SourceFailed(String),
}

/// Streams the source once, hashing it and optionally copying it, within the byte bounds.
fn pump(
    reader: &mut File,
    mut sink: Option<&mut File>,
    budget: &Budget,
) -> Result<Pumped, MigrationError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut read_total = 0_u64;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Ok(Pumped::SourceFailed(error.to_string())),
        };
        read_total += read as u64;
        if read_total > budget.file_limit {
            return Err(uncaptured(ImportRefusal::OverLimit {
                what: "source file bytes",
                limit: budget.file_limit,
                observed: read_total.max(budget.declared),
            }));
        }
        if budget.total_before + read_total > budget.total_limit {
            return Err(uncaptured(ImportRefusal::OverLimit {
                what: "capture bytes",
                limit: budget.total_limit,
                observed: budget.total_before + read_total,
            }));
        }
        hasher.update(&buffer[..read]);
        if let Some(out) = sink.as_mut() {
            out.write_all(&buffer[..read])
                .map_err(|error| io_error("write backup copy", error))?;
        }
    }
    Ok(Pumped::Done(read_total, hasher.finish()))
}

fn stage(
    source: &Path,
    backup: &Path,
    entry: &Entry,
    limits: &CaptureLimits,
    total: &mut u64,
) -> Result<Captured, MigrationError> {
    let unreadable = |kind: EntryKind, detail: String| Captured::Unreadable {
        kind,
        detail: bounded(&detail),
    };
    let path = resolve(source, &entry.relative);
    let (mut handle, meta) = match open_regular(&path) {
        Ok(opened) => opened,
        Err((kind, detail)) => return Ok(unreadable(kind, detail)),
    };
    if meta.len() > limits.max_file_bytes {
        return Err(uncaptured(ImportRefusal::OverLimit {
            what: "source file bytes",
            limit: limits.max_file_bytes,
            observed: meta.len(),
        }));
    }
    let budget = Budget {
        declared: meta.len(),
        file_limit: limits.max_file_bytes,
        total_limit: limits.max_total_bytes,
        total_before: *total,
    };
    let originals = backup.join(ORIGINALS);
    ensure_parents(&originals, &entry.relative)?;
    let target = resolve(&originals, &entry.relative);
    let pumped = if assets::entry_exists(&target)? {
        pump(&mut handle, None, &budget)?
    } else {
        let staged = backup
            .join(STAGING)
            .join(format!("{:032x}", unique_token()));
        let mut out = create_owner_file(&staged)?;
        let pumped = pump(&mut handle, Some(&mut out), &budget)?;
        out.sync_all()
            .map_err(|error| io_error("sync backup copy", error))?;
        drop(out);
        if let Pumped::Done(size, sha256) = &pumped {
            publish_copy(&staged, &target, &entry.relative, (*size, *sha256))?;
        }
        pumped
    };
    let (size, sha256) = match pumped {
        Pumped::Done(size, sha256) => (size, sha256),
        Pumped::SourceFailed(detail) => return Ok(unreadable(EntryKind::Regular, detail)),
    };
    if hash_path(&target, size).ok() != Some((size, sha256)) {
        return Err(uncaptured(ImportRefusal::StagingConflict {
            relative: entry.relative.clone(),
        }));
    }
    set_readonly(&target)?;
    *total += size;
    Ok(Captured::Bytes { size, sha256 })
}

fn ensure_parents(originals: &Path, relative: &str) -> Result<(), StorageError> {
    let parts: Vec<&str> = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let mut dir = originals.to_path_buf();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        dir.push(part);
        ensure_owned_dir(&dir)?;
    }
    Ok(())
}

/// Publishes a staged copy without replacing anything; an existing copy must be identical.
fn publish_copy(
    staged: &Path,
    target: &Path,
    relative: &str,
    expected: (u64, Sha256Digest),
) -> Result<(), MigrationError> {
    let dir = target.parent().unwrap_or(target);
    match publish_no_replace(staged, target, dir) {
        Ok(()) => set_readonly(target),
        Err(StorageError::Io {
            kind: ErrorKind::AlreadyExists,
            ..
        }) if hash_path(target, expected.0).ok() == Some(expected) => set_readonly(target),
        Err(StorageError::Io {
            kind: ErrorKind::AlreadyExists,
            ..
        }) => Err(uncaptured(ImportRefusal::StagingConflict {
            relative: relative.to_string(),
        })),
        Err(error) => Err(error.into()),
    }
}

fn set_readonly(path: &Path) -> Result<(), MigrationError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("inspect backup copy", error))?;
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        return Ok(());
    }
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|error| io_error("seal backup copy", error))?;
    Ok(())
}

/// Writes a manifest once under `relative`; an existing one must hold the same bytes.
pub(crate) fn publish_immutable(
    backup: &Path,
    relative: &str,
    bytes: &[u8],
) -> Result<PathBuf, MigrationError> {
    ensure_parents(backup, relative)?;
    let target = resolve(backup, relative);
    let expected = (bytes.len() as u64, Sha256Digest::of(bytes));
    if assets::entry_exists(&target)? {
        if hash_path(&target, expected.0).ok() == Some(expected) {
            set_readonly(&target)?;
            return Ok(target);
        }
        return Err(uncaptured(ImportRefusal::StagingConflict {
            relative: relative.to_string(),
        }));
    }
    ensure_owned_dir(&backup.join(STAGING))?;
    let staged = backup
        .join(STAGING)
        .join(format!("{:032x}", unique_token()));
    let mut out = create_owner_file(&staged)?;
    out.write_all(bytes)
        .map_err(|error| io_error("write import manifest", error))?;
    out.sync_all()
        .map_err(|error| io_error("sync import manifest", error))?;
    drop(out);
    publish_copy(&staged, &target, relative, expected)?;
    Ok(target)
}

/// The published file under `relative` must still hash to `digest`.
pub(crate) fn verify_immutable(
    backup: &Path,
    relative: &str,
    size: u64,
    digest: Sha256Digest,
) -> Result<(), ImportRefusal> {
    let path = resolve(backup, relative);
    let readonly = fs::symlink_metadata(&path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().readonly());
    if !readonly || hash_path(&path, size).ok() != Some((size, digest)) {
        return Err(ImportRefusal::BackupTampered {
            relative: relative.to_string(),
        });
    }
    Ok(())
}

#[cfg(windows)]
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
#[cfg(windows)]
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// The reparse tag of the entry itself, read without following it.
#[cfg(windows)]
fn reparse_tag(path: &Path) -> Option<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};
    let wide: Vec<u16> = assets::native_path(path)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut found = WIN32_FIND_DATAW::default();
    // SAFETY: `wide` is a NUL-terminated path and `found` a writable result buffer; the
    // returned search handle is closed before leaving.
    let handle = unsafe { FindFirstFileW(wide.as_ptr(), &mut found) };
    if handle as isize == -1 {
        return None;
    }
    // SAFETY: `handle` is the valid search handle opened above.
    unsafe { FindClose(handle) };
    Some(found.dwReserved0)
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn entry_kind(path: &Path, meta: &fs::Metadata) -> EntryKind {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return match reparse_tag(path) {
                Some(IO_REPARSE_TAG_SYMLINK) => EntryKind::Symlink,
                Some(IO_REPARSE_TAG_MOUNT_POINT) => EntryKind::Junction,
                _ => EntryKind::Reparse,
            };
        }
    }
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::Regular
    } else {
        EntryKind::Other
    }
}

/// The platform type bits of a non-regular entry, so two entries of one kind still differ.
fn type_detail(meta: &fs::Metadata) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        format!(
            "not a regular file (attributes {:#x})",
            meta.file_attributes()
        )
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("not a regular file (mode {:o})", meta.mode() & 0o170_000)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = meta;
        "not a regular file".to_string()
    }
}

/// Opens a regular file for reading; the error is the entry's kind and why it cannot be
/// captured.
pub(crate) fn open_regular(path: &Path) -> Result<(File, fs::Metadata), (EntryKind, String)> {
    let meta =
        fs::symlink_metadata(path).map_err(|error| (EntryKind::Unstatable, error.to_string()))?;
    let kind = entry_kind(path, &meta);
    if kind != EntryKind::Regular {
        return Err((kind, type_detail(&meta)));
    }
    let handle = File::open(path).map_err(|error| (kind, error.to_string()))?;
    Ok((handle, meta))
}

/// What a capture of a source entry sees, comparable with what an earlier pass recorded.
#[derive(Debug, PartialEq, Eq)]
enum Observed {
    Bytes(u64, Sha256Digest),
    Unreadable(EntryKind, String),
}

fn observe(path: &Path, expected: u64) -> Observed {
    let handle = match open_regular(path) {
        Ok((handle, _)) => handle,
        Err((kind, detail)) => return Observed::Unreadable(kind, bounded(&detail)),
    };
    match hash_reader(handle, expected) {
        Ok((size, sha256)) => Observed::Bytes(size, sha256),
        Err(error) => Observed::Unreadable(EntryKind::Regular, bounded(&error.to_string())),
    }
}

/// Hashes at most `expected + 1` bytes, so a grown file is seen without reading it all.
fn hash_path(path: &Path, expected: u64) -> std::io::Result<(u64, Sha256Digest)> {
    let (handle, _) = open_regular(path)
        .map_err(|(_, detail)| std::io::Error::new(ErrorKind::InvalidData, detail))?;
    hash_reader(handle, expected)
}

fn hash_reader(handle: File, expected: u64) -> std::io::Result<(u64, Sha256Digest)> {
    let mut handle = handle.take(expected.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut total = 0_u64;
    loop {
        let read = match handle.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        total += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((total, hasher.finish()))
}

/// A second listing and hash pass; any difference means the source moved under the capture.
fn confirm_unchanged(
    source: &Path,
    limits: &CaptureLimits,
    entries: &[Entry],
    files: &[SourceFile],
) -> Result<(), ImportRefusal> {
    let again = scan(source, limits)?;
    let changed = again
        .iter()
        .find(|entry| !entries.contains(entry))
        .or_else(|| entries.iter().find(|entry| !again.contains(entry)));
    if let Some(entry) = changed {
        return Err(ImportRefusal::SourceChanged {
            relative: entry.relative.clone(),
        });
    }
    for file in files {
        let path = resolve(source, &file.relative);
        let unchanged = match &file.captured {
            Captured::Bytes { size, sha256 } => {
                observe(&path, *size) == Observed::Bytes(*size, *sha256)
            }
            Captured::Unreadable { kind, detail } => {
                observe(&path, 0) == Observed::Unreadable(*kind, detail.clone())
            }
        };
        if !unchanged {
            return Err(ImportRefusal::SourceChanged {
                relative: file.relative.clone(),
            });
        }
    }
    Ok(())
}
