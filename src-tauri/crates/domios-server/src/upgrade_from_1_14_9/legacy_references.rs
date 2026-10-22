use super::{legacy_data_path, write_file_atomically};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_DOCUMENT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReferenceKind {
    StoredPath,
    Plugin,
    LaunchAgent,
    SystemdUnit,
    Script,
    UnexpectedWrite,
    ScanFailure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct LegacyReferenceNotice {
    pub kind: ReferenceKind,
    pub file: PathBuf,
    pub fix: String,
    pub modified_ms: Option<u64>,
    fingerprint: String,
}

fn notice(kind: ReferenceKind, file: &Path, fix: String, content: &[u8]) -> LegacyReferenceNotice {
    LegacyReferenceNotice {
        kind,
        file: file.into(),
        fix,
        modified_ms: None,
        fingerprint: format!("{:x}", Sha256::digest(content)),
    }
}

fn normalized(text: &str, windows: bool) -> String {
    if windows {
        text.replace('\\', "/")
            .replace("//", "/")
            .replace("//", "/")
            .to_lowercase()
            .replace("/?/unc/", "/")
            .replace("/?/", "")
    } else {
        text.into()
    }
}

fn contains_root(text: &str, root: &str, windows: bool) -> bool {
    let text = normalized(text, windows);
    let root = normalized(root, windows);
    text.match_indices(&root).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + root.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
            && !after.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
    })
}

fn inspect(
    file: &Path,
    text: &str,
    kind: ReferenceKind,
    legacy: &Path,
    data: &Path,
    windows: bool,
    missing_executable: bool,
) -> Option<LegacyReferenceNotice> {
    let old = legacy.to_string_lossy();
    let home_form = format!("~/{}", legacy.file_name()?.to_str()?);
    let stale_root = contains_root(text, &old, windows) || contains_root(text, &home_form, windows);
    let service = matches!(
        kind,
        ReferenceKind::LaunchAgent | ReferenceKind::SystemdUnit
    );
    let old_service = service
        && (file
            .file_name()?
            .to_string_lossy()
            .to_lowercase()
            .contains("codeterm")
            || text.to_lowercase().contains("codeterm"));
    if !stale_root && !(old_service && missing_executable) {
        return None;
    }
    let quoted_file = shell_quote(&file.to_string_lossy());
    let quoted_unit = shell_quote(&file.file_name()?.to_string_lossy());
    let fix = match kind {
        ReferenceKind::LaunchAgent => format!("Review this user LaunchAgent. Retarget its ProgramArguments, WorkingDirectory and log paths to the installed Domios binaries and {}; update any referenced script too. Unload it with launchctl bootout gui/$(id -u) {quoted_file} before editing, then bootstrap the repaired file. If package autostart already covers it, leave it unloaded and remove this duplicate. Domios ownership is unproven; no file was changed.", data.display()),
        ReferenceKind::SystemdUnit => format!("Review this user unit. Run systemctl --user disable --now {quoted_unit} to stop its restart loop, then retarget ExecStart to the installed Domios desktop binary and paths to {}. Run systemctl --user daemon-reload before re-enabling. If package autostart already covers it, leave this duplicate disabled. Domios ownership is unproven; no file was changed.", data.display()),
        ReferenceKind::Plugin => format!("Update this plugin from its official channel. Replace hardcoded legacy data roots with host.dataDir() in plugin code and ~/.domios in manifest dataPaths; update configured data paths to {}. Plugin bundles are content-trusted and were not rewritten.", data.display()),
        _ => format!("Replace data paths under {} (including {}) with {} in this file; use the running instance data root for writers and codeterm pad path for AgentPad files.", legacy.display(), home_form, data.display()),
    };
    Some(notice(kind, file, fix, text.as_bytes()))
}

fn read_document(file: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(file)?
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::other(
            "document exceeds the 2 MiB startup scan bound",
        ));
    }
    if bytes.starts_with(b"bplist") {
        return Err(io::Error::other(
            "binary plist requires inspection with plutil -p",
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn scan_file(
    notices: &mut Vec<LegacyReferenceNotice>,
    file: &Path,
    kind: ReferenceKind,
    legacy: &Path,
    data: &Path,
) {
    if matches!(
        kind,
        ReferenceKind::LaunchAgent | ReferenceKind::SystemdUnit
    ) && !file.file_name().is_some_and(|name| {
        let name = name.to_string_lossy().to_lowercase();
        name.contains("codeterm") || name.contains("domios")
    }) {
        return;
    }
    match read_document(file) {
        Ok(text) => {
            let missing = missing_service_executable(&text, kind, |path| Path::new(path).is_file());
            notices.extend(inspect(
                file,
                &text,
                kind,
                legacy,
                data,
                cfg!(windows),
                missing,
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => notices.push(scan_failure(file, error)),
    }
}

fn scan_failure(file: &Path, error: impl std::fmt::Display) -> LegacyReferenceNotice {
    let fix = format!("Cannot scan this location: {error}. Inspect it for legacy data paths and stale executables, repair those references, and make it readable for the next startup.");
    notice(
        ReferenceKind::ScanFailure,
        file,
        fix.clone(),
        fix.as_bytes(),
    )
}

fn entries(path: &Path, notices: &mut Vec<LegacyReferenceNotice>) -> Vec<PathBuf> {
    match fs::read_dir(path) {
        Ok(entries) => {
            let mut paths = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => paths.push(entry.path()),
                    Err(error) => notices.push(scan_failure(path, error)),
                }
            }
            paths.sort();
            paths
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            notices.push(scan_failure(path, error));
            Vec::new()
        }
    }
}

pub(crate) fn scan(data: &Path, config: &Path) -> Vec<LegacyReferenceNotice> {
    let Some(legacy) = legacy_data_path(data) else {
        return Vec::new();
    };
    let mut notices = Vec::new();
    for path in [data.join("crons.json"), config.into()] {
        scan_file(
            &mut notices,
            &path,
            ReferenceKind::StoredPath,
            &legacy,
            data,
        );
    }
    for directory in entries(&data.join("plugins"), &mut notices) {
        if !directory.is_dir() {
            continue;
        }
        for name in ["plugin.json", "config.json", "settings.json", "plugin.js"] {
            scan_file(
                &mut notices,
                &directory.join(name),
                ReferenceKind::Plugin,
                &legacy,
                data,
            );
        }
    }
    scan_file(
        &mut notices,
        &data.join("scripts/whisper-shim.py"),
        ReferenceKind::Script,
        &legacy,
        data,
    );
    if data.file_name().is_some_and(|name| name == ".domios") {
        if let Some(home) = data.parent() {
            let user_services = if cfg!(target_os = "macos") {
                Some((
                    home.join("Library/LaunchAgents"),
                    ReferenceKind::LaunchAgent,
                    "plist",
                ))
            } else if cfg!(target_os = "linux") {
                Some((
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute())
                        .unwrap_or_else(|| home.join(".config"))
                        .join("systemd/user"),
                    ReferenceKind::SystemdUnit,
                    "service",
                ))
            } else {
                None
            };
            if let Some((directory, kind, extension)) = user_services {
                for file in entries(&directory, &mut notices) {
                    if file.extension().is_some_and(|ext| ext == extension) {
                        scan_file(&mut notices, &file, kind, &legacy, data);
                    }
                }
            }
        }
    }
    notices.sort_by(|a, b| a.file.cmp(&b.file));
    notices.dedup();
    notices
}

pub(crate) fn report_once(
    data: &Path,
    notices: Vec<LegacyReferenceNotice>,
    mut report: impl FnMut(&LegacyReferenceNotice),
) {
    let ledger = data.join("runtime/legacy-reference-notices.json");
    let previous: BTreeSet<String> = match fs::read(&ledger) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                report(&scan_failure(&ledger, error));
                BTreeSet::new()
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => {
            report(&scan_failure(&ledger, error));
            BTreeSet::new()
        }
    };
    let mut current = BTreeSet::new();
    for notice in notices {
        let key = format!(
            "{:?}:{}:{}:{:?}",
            notice.kind,
            notice.file.display(),
            notice.fingerprint,
            notice.modified_ms
        );
        if !previous.contains(&key) && current.insert(key.clone()) {
            report(&notice);
        }
        current.insert(key);
    }
    if current != previous {
        match serde_json::to_vec(&current)
            .map_err(io::Error::other)
            .and_then(|bytes| write_file_atomically(&ledger, &bytes))
        {
            Ok(()) => {}
            Err(error) => report(&scan_failure(&ledger, error)),
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn first_word(value: &str) -> Option<&str> {
    let value = value.trim_start();
    if let Some(rest) = value.strip_prefix('"') {
        rest.split('"').next()
    } else {
        value.split_whitespace().next()
    }
}

fn missing_service_executable(
    text: &str,
    kind: ReferenceKind,
    exists: impl Fn(&str) -> bool,
) -> bool {
    let program = match kind {
        ReferenceKind::SystemdUnit => text
            .lines()
            .find_map(|line| line.trim().strip_prefix("ExecStart=").and_then(first_word)),
        ReferenceKind::LaunchAgent => ["ProgramArguments", "Program"].iter().find_map(|key| {
            let tail = text.split_once(&format!("<key>{key}</key>"))?.1;
            let value = tail.split_once("<string>")?.1.split_once("</string>")?.0;
            Some(value)
        }),
        _ => None,
    };
    program.is_some_and(|path| {
        path.starts_with('/')
            && !exists(
                &path
                    .replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">"),
            )
    })
}

pub(crate) fn scan_legacy_writes(data: &Path) -> Vec<LegacyReferenceNotice> {
    let Some(legacy) = legacy_data_path(data) else {
        return Vec::new();
    };
    // A real pre-relocation store is not evidence of a post-relocation writer.
    if !data.join("storage/state.db").is_file() {
        return Vec::new();
    }
    let mut notices = Vec::new();
    let mut remaining = 128;
    scan_write_directory(&legacy, &mut notices, &mut remaining, 0);
    notices
}

fn scan_write_directory(
    directory: &Path,
    notices: &mut Vec<LegacyReferenceNotice>,
    remaining: &mut usize,
    depth: usize,
) {
    for path in entries(directory, notices) {
        if *remaining == 0 || depth > 8 {
            notices.push(scan_failure(
                directory,
                "legacy write inventory limit reached; inspect remaining files manually",
            ));
            break;
        }
        *remaining -= 1;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                scan_write_directory(&path, notices, remaining, depth + 1)
            }
            Ok(metadata) if metadata.is_file() => {
                let modified_ms = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|time| u64::try_from(time.as_millis()).ok());
                let mut item = notice(ReferenceKind::UnexpectedWrite, &path, "A file was observed under the legacy root after relocation. Inspect the originating agent, skill or user service; replace literal data roots with the running instance data root and resolve pads with codeterm pad path. This scan does not delete or attribute the file.".into(), &[]);
                item.modified_ms = modified_ms;
                notices.push(item);
            }
            Ok(_) => notices.push(scan_failure(
                &path,
                "legacy entry is a link or special file; inspect it manually",
            )),
            Err(error) => notices.push(scan_failure(&path, error)),
        }
    }
}

#[cfg(test)]
mod tests;
