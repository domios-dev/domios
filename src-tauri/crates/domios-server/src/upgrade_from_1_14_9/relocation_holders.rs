use std::{
    fmt, io,
    path::{Path, PathBuf},
};

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

// Only the Windows holder probe constructs these; other platforms render and test them.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HolderEvidence {
    Executable(PathBuf),
    CommandLine,
    WorkingDirectory(PathBuf),
    OpenFile,
}

impl fmt::Display for HolderEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Executable(path) => write!(formatter, "executable {}", path.display()),
            Self::CommandLine => formatter.write_str("command line references the legacy folder"),
            Self::WorkingDirectory(path) => {
                write!(formatter, "working directory {}", path.display())
            }
            Self::OpenFile => formatter.write_str("open file under the legacy folder"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LegacyRootHolder {
    pub(super) pid: u32,
    pub(super) name: String,
    pub(super) evidence: Vec<HolderEvidence>,
}

impl fmt::Display for LegacyRootHolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (PID {})", self.name, self.pid)?;
        if !self.evidence.is_empty() {
            write!(
                formatter,
                "; {}",
                self.evidence
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct HolderSnapshot {
    pub(super) holders: Vec<LegacyRootHolder>,
    pub(super) warnings: Vec<String>,
    pub(super) complete: bool,
}

impl HolderSnapshot {
    pub(super) fn empty() -> Self {
        Self {
            complete: true,
            ..Self::default()
        }
    }

    fn describe(&self) -> String {
        let mut details = Vec::new();
        if self.holders.is_empty() {
            details.push("no process holder was identified".to_string());
        } else {
            details.push(format!(
                "holders: {}",
                self.holders
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        details.extend(
            self.warnings
                .iter()
                .map(|warning| format!("holder scan: {warning}")),
        );
        if !self.complete {
            details.push("holder scan was incomplete".to_string());
        }
        details.join(". ")
    }
}

pub(super) trait LegacyRootHolderProbe {
    fn inspect(&self, legacy_root: &Path) -> HolderSnapshot;
}

pub(super) struct SystemLegacyRootHolderProbe;

impl LegacyRootHolderProbe for SystemLegacyRootHolderProbe {
    fn inspect(&self, legacy_root: &Path) -> HolderSnapshot {
        #[cfg(windows)]
        {
            windows_probe::inspect(legacy_root)
        }
        #[cfg(not(windows))]
        {
            let _ = legacy_root;
            HolderSnapshot::empty()
        }
    }
}

trait AtomicDirectoryRename {
    fn rename(&self, legacy_root: &Path, domios_root: &Path) -> io::Result<()>;
}

struct SystemAtomicDirectoryRename;

impl AtomicDirectoryRename for SystemAtomicDirectoryRename {
    fn rename(&self, legacy_root: &Path, domios_root: &Path) -> io::Result<()> {
        std::fs::rename(legacy_root, domios_root)
    }
}

pub(super) fn rename_legacy_root(
    legacy_root: &Path,
    domios_root: &Path,
    probe: &dyn LegacyRootHolderProbe,
) -> io::Result<()> {
    rename_legacy_root_with(
        legacy_root,
        domios_root,
        probe,
        &SystemAtomicDirectoryRename,
    )
}

fn rename_legacy_root_with(
    legacy_root: &Path,
    domios_root: &Path,
    probe: &dyn LegacyRootHolderProbe,
    rename: &dyn AtomicDirectoryRename,
) -> io::Result<()> {
    match rename.rename(legacy_root, domios_root) {
        Ok(()) => Ok(()),
        Err(error) if windows_folder_in_use(&error) => {
            let first = probe.inspect(legacy_root);
            let second = (!first.holders.is_empty()).then(|| probe.inspect(legacy_root));
            if second
                .as_ref()
                .is_some_and(|second| should_retry_after_holders_exit(&error, &first, second))
            {
                match rename.rename(legacy_root, domios_root) {
                    Ok(()) => Ok(()),
                    Err(retry_error) => Err(io::Error::new(
                        retry_error.kind(),
                        format!(
                            "{} The single retry after the identified holders disappeared also failed: {retry_error}",
                            windows_in_use_refusal(
                                legacy_root,
                                domios_root,
                                &retry_error,
                                second.as_ref().expect("retry has a holder recheck"),
                            )
                        ),
                    )),
                }
            } else {
                let snapshot = second
                    .as_ref()
                    .map(|second| merge_snapshots(&first, second))
                    .unwrap_or(first);
                Err(io::Error::new(
                    error.kind(),
                    windows_in_use_refusal(legacy_root, domios_root, &error, &snapshot),
                ))
            }
        }
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!(
                "Cannot atomically rename {} to {}: {error}",
                legacy_root.display(),
                domios_root.display()
            ),
        )),
    }
}

pub(super) fn should_retry_after_holders_exit(
    error: &io::Error,
    before: &HolderSnapshot,
    after: &HolderSnapshot,
) -> bool {
    windows_folder_in_use(error)
        && !before.holders.is_empty()
        && after.holders.is_empty()
        && after.complete
        && after.warnings.is_empty()
}

fn windows_folder_in_use(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_ACCESS_DENIED) | Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
    )
}

fn windows_in_use_refusal(
    legacy_root: &Path,
    domios_root: &Path,
    error: &io::Error,
    snapshot: &HolderSnapshot,
) -> String {
    let recovery = if snapshot.holders.is_empty() {
        "No process holder was identified. Close any shell, editor, script, or antivirus process holding a path under the legacy folder, then start Domios again."
    } else {
        "Close the listed processes, then start Domios again."
    };
    format!(
        "Cannot atomically rename {} to {}: Windows reports that the legacy folder is in use or access was denied ({error}). {recovery} {}",
        legacy_root.display(),
        domios_root.display(),
        snapshot.describe()
    )
}

fn merge_snapshots(first: &HolderSnapshot, second: &HolderSnapshot) -> HolderSnapshot {
    let mut merged = first.clone();
    merged.complete &= second.complete;
    for warning in &second.warnings {
        if !merged.warnings.contains(warning) {
            merged.warnings.push(warning.clone());
        }
    }
    for holder in &second.holders {
        merge_holder(&mut merged.holders, holder.clone());
    }
    merged
}

fn merge_holder(holders: &mut Vec<LegacyRootHolder>, holder: LegacyRootHolder) {
    if let Some(existing) = holders.iter_mut().find(|current| current.pid == holder.pid) {
        if existing.name == "Unknown process" && holder.name != "Unknown process" {
            existing.name = holder.name;
        }
        for evidence in holder.evidence {
            if !existing.evidence.contains(&evidence) {
                existing.evidence.push(evidence);
            }
        }
    } else {
        holders.push(holder);
        holders.sort_by_key(|holder| holder.pid);
    }
}

#[cfg(windows)]
mod windows_probe {
    use super::{merge_holder, HolderEvidence, HolderSnapshot, LegacyRootHolder};
    use std::{
        io,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr,
    };
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    use windows_sys::Win32::{Foundation::ERROR_MORE_DATA, System::RestartManager::*};

    const MAX_TREE_ENTRIES: usize = 2048;
    const MAX_REGISTERED_FILES: usize = 256;
    const MAX_HOLDER_PROCESSES: usize = 128;

    pub(super) fn inspect(root: &Path) -> HolderSnapshot {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let mut snapshot = HolderSnapshot::empty();
        inspect_processes(&root, &mut snapshot);
        inspect_open_files(&root, &mut snapshot);
        snapshot.holders.sort_by_key(|holder| holder.pid);
        snapshot
    }

    fn inspect_processes(root: &Path, snapshot: &mut HolderSnapshot) {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        for process in system.processes().values() {
            let mut evidence = Vec::new();
            if let Some(executable) = process.exe() {
                if path_under_root(root, executable) {
                    evidence.push(HolderEvidence::Executable(executable.to_path_buf()));
                }
            }
            if let Some(working_directory) = process.cwd() {
                if path_under_root(root, working_directory) {
                    evidence.push(HolderEvidence::WorkingDirectory(
                        working_directory.to_path_buf(),
                    ));
                }
            }
            if process
                .cmd()
                .iter()
                .any(|argument| command_references_root(argument, root))
            {
                evidence.push(HolderEvidence::CommandLine);
            }
            if evidence.is_empty() {
                continue;
            }
            merge_holder(
                &mut snapshot.holders,
                LegacyRootHolder {
                    pid: process.pid().as_u32(),
                    name: process.name().to_string_lossy().into_owned(),
                    evidence,
                },
            );
        }
    }

    fn path_under_root(root: &Path, candidate: &Path) -> bool {
        let root = normalized(root.to_string_lossy().as_ref());
        let candidate = normalized(candidate.to_string_lossy().as_ref());
        candidate == root
            || candidate
                .strip_prefix(&root)
                .is_some_and(|suffix| suffix.starts_with('\\'))
    }

    fn command_references_root(argument: &std::ffi::OsStr, root: &Path) -> bool {
        let root = normalized(root.to_string_lossy().as_ref());
        let argument = normalized(&argument.to_string_lossy());
        argument.match_indices(&root).any(|(start, _)| {
            let end = start + root.len();
            let before = argument[..start].chars().next_back();
            let after = argument[end..].chars().next();
            before.is_none_or(|value| matches!(value, '=' | '"' | '\'' | ' ' | ':'))
                && after
                    .is_none_or(|value| matches!(value, '\\' | '/' | '"' | '\'' | ' ' | ';' | ','))
        })
    }

    fn normalized(value: &str) -> String {
        let mut value = value.trim_matches('"').replace('/', "\\");
        if let Some(without_verbatim_prefix) = value.strip_prefix("\\\\?\\") {
            value = without_verbatim_prefix.to_string();
        }
        value.to_ascii_lowercase()
    }

    fn inspect_open_files(root: &Path, snapshot: &mut HolderSnapshot) {
        let (files, complete) = registered_files(root, snapshot);
        snapshot.complete &= complete;
        if files.is_empty() {
            return;
        }
        let session = match Session::register(&files) {
            Ok(session) => session,
            Err(error) => {
                snapshot.complete = false;
                snapshot.warnings.push(error);
                return;
            }
        };
        match session.holders() {
            Ok(processes) => {
                for process in processes {
                    let name = wide_name(&process.strAppName);
                    merge_holder(
                        &mut snapshot.holders,
                        LegacyRootHolder {
                            pid: process.Process.dwProcessId,
                            name: if name.is_empty() {
                                "Unknown process".into()
                            } else {
                                name
                            },
                            evidence: vec![HolderEvidence::OpenFile],
                        },
                    );
                }
            }
            Err(error) => {
                snapshot.complete = false;
                snapshot.warnings.push(error);
            }
        }
    }

    fn registered_files(root: &Path, snapshot: &mut HolderSnapshot) -> (Vec<PathBuf>, bool) {
        let mut pending = vec![root.to_path_buf()];
        let mut files = Vec::new();
        let mut entries_seen = 0;
        let mut complete = true;
        'directories: while let Some(directory) = pending.pop() {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    complete = false;
                    snapshot.warnings.push(format!(
                        "cannot inspect {} for open files: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };
            for entry in entries {
                entries_seen += 1;
                if entries_seen > MAX_TREE_ENTRIES {
                    complete = false;
                    snapshot.warnings.push(format!(
                        "open-file scan stopped at {MAX_TREE_ENTRIES} directory entries"
                    ));
                    break 'directories;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        complete = false;
                        snapshot
                            .warnings
                            .push(format!("cannot inspect a legacy-root entry: {error}"));
                        continue;
                    }
                };
                let entry_type = match entry.file_type() {
                    Ok(entry_type) => entry_type,
                    Err(error) => {
                        complete = false;
                        snapshot.warnings.push(format!(
                            "cannot inspect {}: {error}",
                            entry.path().display()
                        ));
                        continue;
                    }
                };
                if entry_type.is_dir() {
                    pending.push(entry.path());
                } else if entry_type.is_file() {
                    if files.len() == MAX_REGISTERED_FILES {
                        complete = false;
                        snapshot.warnings.push(format!(
                            "Restart Manager registration stopped at {MAX_REGISTERED_FILES} files"
                        ));
                        break 'directories;
                    }
                    files.push(entry.path());
                }
            }
        }
        (files, complete)
    }

    struct Session(u32);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                RmEndSession(self.0);
            }
        }
    }

    impl Session {
        fn register(files: &[PathBuf]) -> Result<Self, String> {
            let mut session = 0;
            let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
            rm_result(
                unsafe { RmStartSession(&mut session, 0, key.as_mut_ptr()) },
                "start legacy-root holder discovery",
            )?;
            let session = Self(session);
            let names = files
                .iter()
                .map(|path| {
                    path.as_os_str()
                        .encode_wide()
                        .chain(Some(0))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let pointers = names.iter().map(|name| name.as_ptr()).collect::<Vec<_>>();
            rm_result(
                unsafe {
                    RmRegisterResources(
                        session.0,
                        pointers.len() as u32,
                        pointers.as_ptr(),
                        0,
                        ptr::null(),
                        0,
                        ptr::null(),
                    )
                },
                "register legacy-root files",
            )?;
            Ok(session)
        }

        fn holders(&self) -> Result<Vec<RM_PROCESS_INFO>, String> {
            let mut holders = Vec::new();
            loop {
                let mut needed = 0;
                let mut count = holders.len() as u32;
                let mut reasons = 0;
                let result = unsafe {
                    RmGetList(
                        self.0,
                        &mut needed,
                        &mut count,
                        holders.as_mut_ptr(),
                        &mut reasons,
                    )
                };
                if result == 0 {
                    holders.truncate(count as usize);
                    return Ok(holders);
                }
                if result != ERROR_MORE_DATA {
                    return Err(format!(
                        "enumerate legacy-root open-file holders: {}",
                        io::Error::from_raw_os_error(result as i32)
                    ));
                }
                if needed as usize > MAX_HOLDER_PROCESSES || needed as usize <= holders.len() {
                    return Err(format!(
                        "legacy-root holder list did not grow within the {MAX_HOLDER_PROCESSES}-process limit"
                    ));
                }
                holders.resize(needed as usize, RM_PROCESS_INFO::default());
            }
        }
    }

    fn rm_result(code: u32, operation: &str) -> Result<(), String> {
        if code == 0 {
            Ok(())
        } else {
            Err(format!(
                "{operation}: {}",
                std::io::Error::from_raw_os_error(code as i32)
            ))
        }
    }

    fn wide_name(value: &[u16]) -> String {
        String::from_utf16_lossy(
            &value[..value
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(value.len())],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        rename_legacy_root_with, AtomicDirectoryRename, HolderEvidence, HolderSnapshot,
        LegacyRootHolder, LegacyRootHolderProbe,
    };
    use std::{
        io,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct FixedProbe {
        snapshots: Vec<HolderSnapshot>,
        calls: AtomicUsize,
    }

    impl FixedProbe {
        fn new(snapshots: Vec<HolderSnapshot>) -> Self {
            Self {
                snapshots,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl LegacyRootHolderProbe for FixedProbe {
        fn inspect(&self, _legacy_root: &Path) -> HolderSnapshot {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            self.snapshots
                .get(index)
                .or_else(|| self.snapshots.last())
                .cloned()
                .unwrap_or_else(HolderSnapshot::empty)
        }
    }

    struct FixedRename {
        calls: AtomicUsize,
        failures: usize,
    }

    impl AtomicDirectoryRename for FixedRename {
        fn rename(&self, _legacy_root: &Path, _domios_root: &Path) -> io::Result<()> {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.failures {
                Err(io::Error::from_raw_os_error(5))
            } else {
                Ok(())
            }
        }
    }

    fn open_file_holder() -> HolderSnapshot {
        HolderSnapshot {
            holders: vec![LegacyRootHolder {
                pid: 4400,
                name: "node.exe".into(),
                evidence: vec![HolderEvidence::OpenFile],
            }],
            warnings: Vec::new(),
            complete: true,
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn upgrade_from_1_14_9_refuses_open_legacy_root_with_holder_name() {
        let probe = FixedProbe::new(vec![open_file_holder(), open_file_holder()]);
        let rename = FixedRename {
            calls: AtomicUsize::new(0),
            failures: 1,
        };
        let old = Path::new("C:/fixture/.codeterm");
        let new = Path::new("C:/fixture/.domios");

        let error = rename_legacy_root_with(old, new, &probe, &rename).unwrap_err();
        let reason = error.to_string();

        assert!(reason.contains("node.exe (PID 4400)"));
        assert!(reason.contains("open file under the legacy folder"));
        assert!(reason.contains("Close the listed processes, then start Domios again."));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        assert_eq!(rename.calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(windows)]
    #[test]
    fn upgrade_from_1_14_9_refuses_open_legacy_root_with_holder_name() {
        use super::{rename_legacy_root_with, SystemAtomicDirectoryRename};
        use std::os::windows::fs::OpenOptionsExt;

        let fixture = tempfile::tempdir().unwrap();
        let old = fixture.path().join(".codeterm");
        let new = fixture.path().join(".domios");
        std::fs::create_dir(&old).unwrap();
        let handle = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .share_mode(0)
            .open(old.join("held.db"))
            .unwrap();
        let probe = FixedProbe::new(vec![open_file_holder(), open_file_holder()]);

        let error =
            rename_legacy_root_with(&old, &new, &probe, &SystemAtomicDirectoryRename).unwrap_err();
        let reason = error.to_string();

        assert!(reason.contains("Windows reports that the legacy folder is in use"));
        assert!(reason.contains("node.exe (PID 4400)"));
        assert!(reason.contains("open file under the legacy folder"));
        assert!(reason.contains("Close the listed processes, then start Domios again."));
        assert!(old.exists());
        assert!(!new.exists());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        drop(handle);
    }

    #[test]
    fn upgrade_from_1_14_9_retries_once_after_holder_disappears() {
        let probe = FixedProbe::new(vec![open_file_holder(), HolderSnapshot::empty()]);
        let rename = FixedRename {
            calls: AtomicUsize::new(0),
            failures: 1,
        };
        let old = Path::new("C:/fixture/.codeterm");
        let new = Path::new("C:/fixture/.domios");

        rename_legacy_root_with(old, new, &probe, &rename).unwrap();

        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        assert_eq!(rename.calls.load(Ordering::SeqCst), 2);
    }
}
