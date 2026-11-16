use std::io::{Cursor, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

use crate::diagnostics::{DiagnosticCode, DiagnosticSeverity, PackageDiagnostic};
use crate::{paths, Limits};

pub(crate) type Entries = Vec<(String, Vec<u8>)>;

pub(crate) fn entry_error(
    file: &str,
    code: DiagnosticCode,
    message: impl Into<String>,
) -> PackageDiagnostic {
    PackageDiagnostic {
        code,
        severity: DiagnosticSeverity::Error,
        file: file.to_string(),
        path: String::new(),
        line: None,
        column: None,
        message: message.into(),
    }
}

/// Reads a `.team` ZIP into entries; links, encrypted entries and oversize data are refused.
pub(crate) fn read_zip(
    bytes: &[u8],
    limits: &Limits,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> Option<Entries> {
    let mut archive = match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => archive,
        Err(error) => {
            diagnostics.push(entry_error(
                "",
                DiagnosticCode::ArchiveError,
                format!("not a readable ZIP archive: {error}"),
            ));
            return None;
        }
    };
    if archive.len() > limits.max_entries {
        diagnostics.push(entry_error(
            "",
            DiagnosticCode::TooManyEntries,
            format!("the archive holds more than {} entries", limits.max_entries),
        ));
        return None;
    }
    let mut entries = Vec::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let label = archive
            .name_for_index(index)
            .and_then(Result::ok)
            .map(|name| name.into_owned())
            .unwrap_or_else(|| format!("#{index}"));
        let mut file = match archive.by_index(index) {
            Ok(file) => file,
            Err(error) => {
                diagnostics.push(entry_error(
                    &label,
                    DiagnosticCode::UnsupportedEntry,
                    format!("entry cannot be read: {error}"),
                ));
                continue;
            }
        };
        if file.is_symlink() {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::LinkedEntry,
                "links are not allowed in a package",
            ));
            continue;
        }
        if file.is_dir() {
            continue;
        }
        if let Err((code, message)) = paths::check(&label, limits.max_path_bytes) {
            diagnostics.push(entry_error(&label, code, message));
            continue;
        }
        if file.size() > limits.max_entry_bytes as u64 {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::EntryTooLarge,
                format!("entry exceeds {} bytes", limits.max_entry_bytes),
            ));
            continue;
        }
        let mut data = Vec::new();
        let read = Read::by_ref(&mut file)
            .take(limits.max_entry_bytes as u64 + 1)
            .read_to_end(&mut data);
        if let Err(error) = read {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::UnsupportedEntry,
                format!("entry cannot be read: {error}"),
            ));
            continue;
        }
        if data.len() > limits.max_entry_bytes {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::EntryTooLarge,
                format!("entry exceeds {} bytes", limits.max_entry_bytes),
            ));
            continue;
        }
        total += data.len();
        if total > limits.max_total_bytes {
            diagnostics.push(entry_error(
                "",
                DiagnosticCode::TooLarge,
                format!("the package exceeds {} bytes", limits.max_total_bytes),
            ));
            return None;
        }
        entries.push((label, data));
    }
    Some(entries)
}

/// Deterministic ZIP: given order, stored entries, fixed time, Unix 0644 metadata.
pub(crate) fn write_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::DEFAULT)
        .system(System::Unix)
        .unix_permissions(0o644);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer
            .start_file(name.as_str(), options)
            .map_err(|error| error.to_string())?;
        writer.write_all(data).map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|error| error.to_string())
}

fn walk(
    root: &Path,
    dir: &Path,
    limits: &Limits,
    entries: &mut Entries,
    total: &mut usize,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> Result<(), ()> {
    let listing = std::fs::read_dir(dir).map_err(|error| {
        diagnostics.push(entry_error(
            "",
            DiagnosticCode::Unreadable,
            format!("{}: {error}", dir.display()),
        ));
    })?;
    for item in listing {
        let item = item.map_err(|error| {
            diagnostics.push(entry_error(
                "",
                DiagnosticCode::Unreadable,
                error.to_string(),
            ));
        })?;
        let path = item.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let mut parts = Vec::new();
        for component in relative.components() {
            match component.as_os_str().to_str() {
                Some(part) => parts.push(part.to_string()),
                None => {
                    diagnostics.push(entry_error(
                        &relative.display().to_string(),
                        DiagnosticCode::InvalidPath,
                        "file names must be UTF-8",
                    ));
                    return Err(());
                }
            }
        }
        let label = parts.join("/");
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::Unreadable,
                error.to_string(),
            ));
        })?;
        if metadata.file_type().is_symlink() {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::LinkedEntry,
                "links are not allowed in a package",
            ));
            continue;
        }
        if metadata.is_dir() {
            walk(root, &path, limits, entries, total, diagnostics)?;
            continue;
        }
        if entries.len() >= limits.max_entries {
            diagnostics.push(entry_error(
                "",
                DiagnosticCode::TooManyEntries,
                format!("the folder holds more than {} files", limits.max_entries),
            ));
            return Err(());
        }
        if metadata.len() > limits.max_entry_bytes as u64 {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::EntryTooLarge,
                format!("file exceeds {} bytes", limits.max_entry_bytes),
            ));
            continue;
        }
        if let Err((code, message)) = paths::check(&label, limits.max_path_bytes) {
            diagnostics.push(entry_error(&label, code, message));
            continue;
        }
        let data = std::fs::read(&path).map_err(|error| {
            diagnostics.push(entry_error(
                &label,
                DiagnosticCode::Unreadable,
                error.to_string(),
            ));
        })?;
        *total += data.len();
        if *total > limits.max_total_bytes {
            diagnostics.push(entry_error(
                "",
                DiagnosticCode::TooLarge,
                format!("the package exceeds {} bytes", limits.max_total_bytes),
            ));
            return Err(());
        }
        entries.push((label, data));
    }
    Ok(())
}

/// Reads a package folder; links are reported and never followed.
pub(crate) fn read_folder(
    dir: &Path,
    limits: &Limits,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> Option<Entries> {
    let mut entries = Vec::new();
    let mut total = 0;
    walk(dir, dir, limits, &mut entries, &mut total, diagnostics).ok()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Some(entries)
}

/// Writes entries under `dir`, creating parent folders.
pub(crate) fn write_folder(dir: &Path, entries: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    for (name, data) in entries {
        let target = name
            .split('/')
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, data)?;
    }
    Ok(())
}
