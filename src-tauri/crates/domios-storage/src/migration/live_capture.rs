use super::manifest::{bounded, uncaptured, ImportRefusal, MigrationError};
use super::source_inventory;
#[cfg(feature = "test-support")]
use super::CaptureStep;
use super::{
    CaptureLimits, Captured, EntryKind, ImportDomain, ImportOutcome, ImportPlan, LegacyDecoder,
    Sha256Digest, SourceCapture, SourceFile, SourceRole, Storage, IMPORT_MANIFEST_FORMAT,
};
use crate::assets::{self, canonical_dir};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bounds and explicit choices for the one-time import from this Storage's live root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRootImportRequest {
    pub limits: CaptureLimits,
    pub selections: BTreeMap<super::ImportDomain, super::SourceSelection>,
    pub deadline: Duration,
}

impl Default for LiveRootImportRequest {
    fn default() -> Self {
        LiveRootImportRequest {
            limits: CaptureLimits::default(),
            selections: BTreeMap::new(),
            deadline: Duration::from_secs(30),
        }
    }
}

impl LiveRootImportRequest {
    pub fn new() -> Self {
        Self::default()
    }
}

pub(super) fn capture_or_load_live_sources(
    source_root: &Path,
    limits: CaptureLimits,
) -> Result<SourceCapture, MigrationError> {
    let source = canonical_dir(source_root).map_err(|error| {
        uncaptured(ImportRefusal::Unreadable {
            relative: String::new(),
            detail: bounded(&error.to_string()),
        })
    })?;
    let backup = source_inventory::live_backup_dir(&source)?;
    if assets::entry_exists(&backup.join(source_inventory::CAPTURE_MANIFEST))? {
        return load_live_capture(&source, &backup, limits);
    }
    source_inventory::capture_live_sources(&source, limits)
}

fn load_live_capture(
    source: &Path,
    backup: &Path,
    limits: CaptureLimits,
) -> Result<SourceCapture, MigrationError> {
    limits.validate().map_err(uncaptured)?;
    let manifest_path = backup.join(source_inventory::CAPTURE_MANIFEST);
    let tampered = || ImportRefusal::BackupTampered {
        relative: source_inventory::CAPTURE_MANIFEST.to_string(),
    };
    let (file, metadata) =
        source_inventory::open_regular(&manifest_path).map_err(|_| uncaptured(tampered()))?;
    let bound = limits
        .max_files
        .checked_mul(2048)
        .and_then(|value| value.checked_add(source.to_string_lossy().len().saturating_mul(3)))
        .and_then(|value| value.checked_add(2048))
        .ok_or_else(|| uncaptured(tampered()))? as u64;
    if metadata.len() > bound {
        return Err(uncaptured(tampered()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(bound.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| uncaptured(tampered()))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(uncaptured(tampered()));
    }
    let files =
        parse_capture_manifest(&bytes, source, limits).map_err(|_| uncaptured(tampered()))?;
    SourceCapture::from_live_archive(backup, files, &bytes)
}

fn parse_capture_manifest(
    bytes: &[u8],
    source: &Path,
    limits: CaptureLimits,
) -> Result<Vec<SourceFile>, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let mut lines = text.lines();
    let expected_header = format!("codeterm-legacy-capture {IMPORT_MANIFEST_FORMAT}");
    if lines.next() != Some(expected_header.as_str()) {
        return Err(());
    }
    let source_line = lines.next().ok_or(())?;
    let source_token = source_line.strip_prefix("source-root ").ok_or(())?;
    let source_text = decode_token(source_token).ok_or(())?;
    if folded(Path::new(&source_text)) != folded(source) {
        return Err(());
    }
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut previous: Option<(ImportDomain, SourceRole, String)> = None;
    while let Some(line) = lines.next() {
        let words: Vec<&str> = line.split(' ').collect();
        if words.first() == Some(&"end") {
            if words.len() != 2 || words[1].parse::<usize>().ok() != Some(files.len()) {
                return Err(());
            }
            if files.len() > limits.max_files || lines.next().is_some() {
                return Err(());
            }
            return Ok(files);
        }
        if words.first() != Some(&"file") || words.len() < 6 {
            return Err(());
        }
        let domain = parse_domain(words[1]).ok_or(())?;
        let role = parse_role(words[2]).ok_or(())?;
        let relative = decode_token(words[3]).ok_or(())?;
        if !source_inventory::valid_live_inventory_path(domain, role, &relative) {
            return Err(());
        }
        let captured = if words[4] == "unreadable" {
            if words.len() != 7 {
                return Err(());
            }
            Captured::Unreadable {
                kind: parse_kind(words[5]).ok_or(())?,
                detail: decode_token(words[6]).ok_or(())?,
            }
        } else {
            if words.len() != 6 {
                return Err(());
            }
            let size = words[4].parse::<u64>().map_err(|_| ())?;
            let sha256 = Sha256Digest::parse_hex(words[5]).ok_or(())?;
            if size > limits.max_file_bytes {
                return Err(());
            }
            total = total.checked_add(size).ok_or(())?;
            if total > limits.max_total_bytes {
                return Err(());
            }
            Captured::Bytes { size, sha256 }
        };
        let key = (domain, role, relative.clone());
        if previous.as_ref().is_some_and(|prior| prior >= &key) {
            return Err(());
        }
        previous = Some(key);
        files.push(SourceFile {
            domain,
            role,
            relative,
            captured,
        });
        if files.len() > limits.max_files {
            return Err(());
        }
    }
    Err(())
}

fn decode_token(token: &str) -> Option<String> {
    if !token.is_ascii() {
        return None;
    }
    let bytes = token.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = hex_value(*bytes.get(index + 1)?)?;
        let low = hex_value(*bytes.get(index + 2)?)?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_domain(text: &str) -> Option<ImportDomain> {
    match text {
        "desk" => Some(ImportDomain::Desk),
        "cron" => Some(ImportDomain::Cron),
        "blueprint" | "sentinel" => Some(ImportDomain::Sentinel),
        _ => None,
    }
}

fn parse_role(text: &str) -> Option<SourceRole> {
    match text {
        "current" => Some(SourceRole::Current),
        "rotating-backup" => Some(SourceRole::RotatingBackup),
        "named-snapshot" => Some(SourceRole::NamedSnapshot),
        "quarantined" => Some(SourceRole::Quarantined),
        "interrupted-write" => Some(SourceRole::InterruptedWrite),
        "unrecognized" => Some(SourceRole::Unrecognized),
        _ => None,
    }
}

fn parse_kind(text: &str) -> Option<EntryKind> {
    match text {
        "regular" => Some(EntryKind::Regular),
        "directory" => Some(EntryKind::Directory),
        "symlink" => Some(EntryKind::Symlink),
        "junction" => Some(EntryKind::Junction),
        "reparse" => Some(EntryKind::Reparse),
        "other" => Some(EntryKind::Other),
        "unstatable" => Some(EntryKind::Unstatable),
        _ => None,
    }
}

fn folded(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

impl Storage {
    /// Captures the legacy inventory into this root's sealed archive before importing it.
    pub fn import_live_root(
        &self,
        request: &LiveRootImportRequest,
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
        let capture = capture_or_load_live_sources(&root, request.limits)?;
        let plan = ImportPlan::resolve(capture, decoder, &request.selections)?;
        let receipt = self.commit_legacy_import(plan, request.deadline)?;
        Ok(ImportOutcome::Committed(Box::new(receipt)))
    }

    #[cfg(feature = "test-support")]
    pub fn capture_live_root_observed(
        &self,
        limits: CaptureLimits,
        deadline: Duration,
        observer: &mut dyn FnMut(&str) -> CaptureStep,
    ) -> Result<SourceCapture, MigrationError> {
        if deadline.is_zero() {
            return Err(uncaptured(ImportRefusal::InvalidRequest {
                reason: "the deadline must be positive",
            }));
        }
        let root = self.instance_root(deadline)?;
        SourceCapture::capture_live_observed(&root, limits, observer)
    }
}
