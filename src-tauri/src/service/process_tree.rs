//! A tab's own PTY process tree, each process paired with its OS start time.
//! The start time is the PID-reuse guard for every process and every parent link.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
use std::collections::VecDeque;

use crate::agent::provider::LaunchProcess;
use parking_lot::Mutex;

const MAX_DEPTH: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessEntry {
    pid: u32,
    parent_pid: u32,
    image_name: String,
}

/// Descendants of `root` (itself included), shallowest first.
pub(crate) fn launch_processes(root: u32) -> Vec<LaunchProcess> {
    launch_tree(root, &platform::snapshot(), platform::start_time)
}

pub(crate) fn may_have_named_process(root: u32, expected_name: &str) -> bool {
    let entries = platform::snapshot();
    tree_may_have_named_process(root, &entries, platform::start_time, expected_name)
}

pub(crate) fn has_named_process(root: u32, expected_name: &str) -> bool {
    let entries = platform::snapshot();
    tree_has_named_process(root, &entries, platform::start_time, expected_name)
}

fn tree_has_named_process(
    root: u32,
    entries: &[ProcessEntry],
    start_time: impl Fn(u32) -> Option<u64>,
    expected_name: &str,
) -> bool {
    let pids = launch_tree(root, entries, start_time)
        .into_iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    entries.iter().any(|entry| {
        pids.contains(&entry.pid) && executable_names_match(&entry.image_name, expected_name)
    })
}

pub(crate) struct ProviderTreeWitnesses {
    observations: Mutex<HashMap<ProviderTreeKey, ProviderTreeHistory>>,
    #[cfg(test)]
    scripted: Mutex<HashMap<ProviderTreeKey, VecDeque<ProviderProcessTreeSnapshot>>>,
}

impl Default for ProviderTreeWitnesses {
    fn default() -> Self {
        Self {
            observations: Mutex::new(HashMap::new()),
            #[cfg(test)]
            scripted: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderTreeState {
    Present,
    Unknown,
    AbsentBeforeSeen,
    GoneAfterSeen,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProviderTreeKey {
    pub(crate) tab_id: String,
    pub(crate) pty_id: String,
    pub(crate) binding_nonce: Option<String>,
    pub(crate) executable_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProviderTreeObservation {
    key: ProviderTreeKey,
    state: ProviderTreeState,
    provider_seen_before: bool,
    consecutive_absent_observations: usize,
}

impl ProviderTreeObservation {
    pub(crate) fn key(&self) -> &ProviderTreeKey {
        &self.key
    }

    pub(crate) fn state(&self) -> ProviderTreeState {
        self.state
    }

    pub(crate) fn proves_exit_from_finished_marker(&self) -> bool {
        self.provider_seen_before && self.state == ProviderTreeState::GoneAfterSeen
    }

    pub(crate) fn proves_exit_from_lifecycle_clear(&self) -> bool {
        self.state == ProviderTreeState::GoneAfterSeen
    }

    pub(crate) fn has_distinct_absence_observations(&self) -> bool {
        self.consecutive_absent_observations >= 2
    }

    pub(crate) fn evidence(&self) -> &'static str {
        match self.state {
            ProviderTreeState::Present => "provider_present_in_tab_tree",
            ProviderTreeState::Unknown => "tab_tree_unavailable_or_incomplete",
            ProviderTreeState::AbsentBeforeSeen => "provider_not_seen_before_absence",
            ProviderTreeState::GoneAfterSeen => "provider_seen_then_absent_snapshot",
        }
    }
}

#[derive(Default)]
struct ProviderTreeHistory {
    seen_processes: HashSet<ProcessIdentity>,
    consecutive_absent_observations: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) start_token: String,
}

pub(crate) enum ProviderProcessTreeSnapshot {
    Present(HashSet<ProcessIdentity>),
    Absent,
    Unknown,
}

impl ProviderTreeWitnesses {
    pub(crate) fn observe(
        &self,
        key: &ProviderTreeKey,
        root: u32,
        expected_name: &str,
    ) -> ProviderTreeObservation {
        #[cfg(test)]
        if let Some(snapshot) = self
            .scripted
            .lock()
            .get_mut(key)
            .and_then(VecDeque::pop_front)
        {
            return self.record(key, snapshot);
        }
        let observation = named_process_snapshot(root, expected_name);
        self.record(key, observation)
    }

    #[cfg(test)]
    pub(crate) fn script_snapshot(
        &self,
        key: &ProviderTreeKey,
        snapshot: ProviderProcessTreeSnapshot,
    ) {
        self.scripted
            .lock()
            .entry(key.clone())
            .or_default()
            .push_back(snapshot);
    }

    pub(crate) fn unavailable(&self, key: &ProviderTreeKey) -> ProviderTreeObservation {
        self.record(key, ProviderProcessTreeSnapshot::Unknown)
    }

    #[cfg(test)]
    pub(crate) fn has_seen(&self, key: &ProviderTreeKey) -> bool {
        self.observations
            .lock()
            .get(key)
            .is_some_and(|history| !history.seen_processes.is_empty())
    }

    fn record(
        &self,
        key: &ProviderTreeKey,
        observation: ProviderProcessTreeSnapshot,
    ) -> ProviderTreeObservation {
        let mut observations = self.observations.lock();
        observations.retain(|current, _| current.tab_id != key.tab_id || current == key);
        let history = observations.entry(key.clone()).or_default();
        let provider_seen_before = !history.seen_processes.is_empty();
        let state = match observation {
            ProviderProcessTreeSnapshot::Present(processes) => {
                history.consecutive_absent_observations = 0;
                history.seen_processes.extend(processes);
                ProviderTreeState::Present
            }
            ProviderProcessTreeSnapshot::Absent => {
                history.consecutive_absent_observations += 1;
                if provider_seen_before {
                    ProviderTreeState::GoneAfterSeen
                } else {
                    ProviderTreeState::AbsentBeforeSeen
                }
            }
            ProviderProcessTreeSnapshot::Unknown => {
                history.consecutive_absent_observations = 0;
                ProviderTreeState::Unknown
            }
        };
        ProviderTreeObservation {
            key: key.clone(),
            state,
            provider_seen_before,
            consecutive_absent_observations: history.consecutive_absent_observations,
        }
    }

    #[cfg(test)]
    pub(crate) fn record_for_test(
        &self,
        key: &ProviderTreeKey,
        observation: ProviderProcessTreeSnapshot,
    ) -> ProviderTreeObservation {
        self.record(key, observation)
    }

    pub(crate) fn clear_tab(&self, tab_id: &str) {
        self.observations
            .lock()
            .retain(|key, _| key.tab_id != tab_id);
    }
}

fn named_process_snapshot(root: u32, expected_name: &str) -> ProviderProcessTreeSnapshot {
    let entries = platform::snapshot();
    named_process_snapshot_from_entries(root, &entries, platform::start_time, expected_name)
}

fn named_process_snapshot_from_entries(
    root: u32,
    entries: &[ProcessEntry],
    start_time: impl Fn(u32) -> Option<u64>,
    expected_name: &str,
) -> ProviderProcessTreeSnapshot {
    let tree = launch_tree(root, entries, &start_time);
    let pids = tree
        .iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    if !pids.contains(&root) {
        return ProviderProcessTreeSnapshot::Unknown;
    }
    let matching = tree
        .iter()
        .filter(|process| {
            entries
                .iter()
                .find(|entry| entry.pid == process.pid)
                .is_some_and(|entry| executable_names_match(&entry.image_name, expected_name))
        })
        .map(|process| ProcessIdentity {
            pid: process.pid,
            start_token: process.start_token.clone(),
        })
        .collect::<HashSet<_>>();
    if !matching.is_empty() {
        return ProviderProcessTreeSnapshot::Present(matching);
    }
    if tree.iter().any(|process| {
        entries
            .iter()
            .find(|entry| entry.pid == process.pid)
            .is_none_or(|entry| entry.image_name.is_empty())
    }) || entries.iter().any(|entry| {
        pids.contains(&entry.parent_pid)
            && !pids.contains(&entry.pid)
            && executable_names_match(&entry.image_name, expected_name)
            && start_time(entry.pid).is_none()
    }) {
        ProviderProcessTreeSnapshot::Unknown
    } else {
        ProviderProcessTreeSnapshot::Absent
    }
}

fn tree_may_have_named_process(
    root: u32,
    entries: &[ProcessEntry],
    start_time: impl Fn(u32) -> Option<u64>,
    expected_name: &str,
) -> bool {
    !matches!(
        named_process_snapshot_from_entries(root, entries, start_time, expected_name),
        ProviderProcessTreeSnapshot::Absent
    )
}

fn executable_names_match(actual: &str, expected: &str) -> bool {
    fn base(name: &str) -> &str {
        let name = name.rsplit(['\\', '/']).next().unwrap_or(name);
        name.get(name.len().saturating_sub(4)..)
            .filter(|suffix| suffix.eq_ignore_ascii_case(".exe"))
            .map_or(name, |_| &name[..name.len() - 4])
    }
    base(actual).eq_ignore_ascii_case(base(expected))
}

fn launch_tree(
    root: u32,
    entries: &[ProcessEntry],
    start_time: impl Fn(u32) -> Option<u64>,
) -> Vec<LaunchProcess> {
    let mut starts: HashMap<u32, u64> = HashMap::new();
    let mut out: Vec<LaunchProcess> = Vec::new();
    let Some(root_start) = start_time(root) else {
        return out;
    };
    starts.insert(root, root_start);
    out.push(LaunchProcess {
        pid: root,
        depth: 0,
        start_token: root_start.to_string(),
    });
    let mut index = 0;
    while index < out.len() {
        let (parent, depth) = (out[index].pid, out[index].depth);
        index += 1;
        if depth >= MAX_DEPTH {
            continue;
        }
        let parent_start = starts[&parent];
        for entry in entries.iter().filter(|entry| entry.parent_pid == parent) {
            if starts.contains_key(&entry.pid) {
                continue;
            }
            // A child older than its parent names a reused parent pid, not a child.
            let Some(start) = start_time(entry.pid).filter(|start| *start >= parent_start) else {
                continue;
            };
            starts.insert(entry.pid, start);
            out.push(LaunchProcess {
                pid: entry.pid,
                depth: depth + 1,
                start_token: start.to_string(),
            });
        }
    }
    out
}

#[cfg(windows)]
mod platform {
    use super::ProcessEntry;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub(super) fn snapshot() -> Vec<ProcessEntry> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entries = Vec::new();
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more {
            entries.push(ProcessEntry {
                pid: entry.th32ProcessID,
                parent_pid: entry.th32ParentProcessID,
                image_name: {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|unit| *unit == 0)
                        .unwrap_or(entry.szExeFile.len());
                    String::from_utf16_lossy(&entry.szExeFile[..end])
                },
            });
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe {
            CloseHandle(snapshot);
        }
        entries
    }

    /// Creation time as a FILETIME count, the encoding Windows tools record.
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let read =
            unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) }
                != 0;
        unsafe {
            CloseHandle(handle);
        }
        read.then(|| (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ProcessEntry;

    pub(super) fn snapshot() -> Vec<ProcessEntry> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                let (parent_pid, _) = stat_fields(pid)?;
                Some(ProcessEntry {
                    pid,
                    parent_pid,
                    image_name: std::fs::read_to_string(format!("/proc/{pid}/comm"))
                        .ok()
                        .map(|name| name.trim().to_string())
                        .unwrap_or_default(),
                })
            })
            .collect()
    }

    /// `starttime` from `/proc/<pid>/stat`, in clock ticks since boot.
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        stat_fields(pid).map(|(_, start)| start)
    }

    fn stat_fields(pid: u32) -> Option<(u32, u64)> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields: Vec<&str> = stat[stat.rfind(')')? + 1..].split_whitespace().collect();
        Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ProcessEntry;

    pub(super) fn snapshot() -> Vec<ProcessEntry> {
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Vec::new();
        }
        let mut pids = vec![0 as libc::pid_t; count as usize * 2];
        let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if filled <= 0 {
            return Vec::new();
        }
        pids.truncate(filled as usize);
        pids.into_iter()
            .filter(|pid| *pid > 0)
            .filter_map(|pid| {
                let info = bsd_info(pid as u32)?;
                Some(ProcessEntry {
                    pid: pid as u32,
                    parent_pid: info.pbi_ppid,
                    image_name: String::from_utf8_lossy(
                        &info
                            .pbi_name
                            .iter()
                            .take_while(|unit| **unit != 0)
                            .map(|unit| *unit as u8)
                            .collect::<Vec<_>>(),
                    )
                    .into_owned(),
                })
            })
            .collect()
    }

    /// Start time in microseconds since the epoch.
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        let info = bsd_info(pid)?;
        info.pbi_start_tvsec
            .checked_mul(1_000_000)?
            .checked_add(info.pbi_start_tvusec)
    }

    fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (written == size).then_some(info)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::ProcessEntry;

    pub(super) fn snapshot() -> Vec<ProcessEntry> {
        Vec::new()
    }

    pub(super) fn start_time(_: u32) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pid: u32, parent_pid: u32) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            image_name: String::new(),
        }
    }

    fn named_entry(pid: u32, parent_pid: u32, image_name: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            image_name: image_name.into(),
        }
    }

    fn tree_key() -> ProviderTreeKey {
        ProviderTreeKey {
            tab_id: "tab-tree-witness".into(),
            pty_id: "pty-tree-witness".into(),
            binding_nonce: Some("nonce-tree-witness".into()),
            executable_name: "opencode".into(),
        }
    }

    fn provider_process(pid: u32) -> HashSet<ProcessIdentity> {
        HashSet::from([ProcessIdentity {
            pid,
            start_token: format!("start-{pid}"),
        }])
    }

    fn pids(tree: &[LaunchProcess]) -> Vec<(u32, u32)> {
        tree.iter()
            .map(|process| (process.pid, process.depth))
            .collect()
    }

    #[test]
    fn tree_lists_only_the_roots_descendants_shallowest_first() {
        let entries = [entry(20, 10), entry(30, 20), entry(40, 99), entry(21, 10)];
        let tree = launch_tree(10, &entries, |pid| Some(u64::from(pid)));
        assert_eq!(pids(&tree), vec![(10, 0), (20, 1), (21, 1), (30, 2)]);
        assert_eq!(tree[1].start_token, "20");
    }

    #[test]
    fn child_older_than_its_parent_is_a_reused_pid_and_is_excluded() {
        let entries = [entry(20, 10), entry(30, 20)];
        let starts = |pid: u32| match pid {
            10 => Some(500),
            20 => Some(100),
            _ => Some(900),
        };
        assert_eq!(pids(&launch_tree(10, &entries, starts)), vec![(10, 0)]);
    }

    #[test]
    fn provider_process_must_be_named_and_descend_from_the_tab_root() {
        let entries = [
            named_entry(10, 1, "powershell.exe"),
            named_entry(20, 10, "codex.exe"),
            named_entry(30, 1, "codex.exe"),
            named_entry(40, 1, "powershell.exe"),
        ];

        assert!(tree_has_named_process(10, &entries, |pid| Some(pid.into()), "codex"));
        assert!(!tree_has_named_process(40, &entries, |pid| Some(pid.into()), "codex"));
    }

    #[test]
    fn named_process_lookup_is_scoped_to_the_tab_tree_and_normalizes_exe_suffix() {
        let entries = [
            named_entry(10, 1, "pwsh.exe"),
            named_entry(20, 10, "opencode.exe"),
            named_entry(30, 99, "pwsh.exe"),
            named_entry(40, 99, "opencode.exe"),
        ];
        let has_process = |root, expected| {
            launch_tree(root, &entries, |pid| Some(u64::from(pid)))
                .iter()
                .any(|process| {
                    entries
                        .iter()
                        .find(|entry| entry.pid == process.pid)
                        .is_some_and(|entry| executable_names_match(&entry.image_name, expected))
                })
        };

        assert!(has_process(10, "opencode"));
        assert!(has_process(10, "OPENCODE.EXE"));
        assert!(!has_process(30, "opencode"));
    }

    #[test]
    fn tree_witnesses_do_not_treat_first_absence_as_exit() {
        let witnesses = ProviderTreeWitnesses::default();
        let key = tree_key();
        let first_absence = witnesses.record(&key, ProviderProcessTreeSnapshot::Absent);

        assert_eq!(first_absence.state, ProviderTreeState::AbsentBeforeSeen);
        assert!(!first_absence.proves_exit_from_lifecycle_clear());
        assert!(!first_absence.proves_exit_from_finished_marker());

        let live = witnesses.record(
            &key,
            ProviderProcessTreeSnapshot::Present(provider_process(77)),
        );
        assert_eq!(live.state, ProviderTreeState::Present);
        assert!(!live.proves_exit_from_lifecycle_clear());

        let one_absence = witnesses.record(&key, ProviderProcessTreeSnapshot::Absent);
        assert_eq!(one_absence.state, ProviderTreeState::GoneAfterSeen);
        assert!(one_absence.proves_exit_from_finished_marker());
        assert!(one_absence.proves_exit_from_lifecycle_clear());
    }

    #[test]
    fn incomplete_tree_and_live_provider_are_not_exit_evidence() {
        let entries = [
            named_entry(10, 1, "powershell.exe"),
            named_entry(20, 10, "opencode.exe"),
        ];
        assert!(matches!(
            named_process_snapshot_from_entries(10, &entries, |pid| Some(u64::from(pid)), "opencode"),
            ProviderProcessTreeSnapshot::Present(processes) if processes.contains(&ProcessIdentity { pid: 20, start_token: "20".into() })
        ));
        assert!(matches!(
            named_process_snapshot_from_entries(10, &[], |_| None, "opencode"),
            ProviderProcessTreeSnapshot::Unknown
        ));
    }

    #[test]
    fn missing_root_or_child_identity_is_conservatively_live() {
        let entries = [
            named_entry(10, 1, "pwsh.exe"),
            named_entry(20, 10, "opencode.exe"),
        ];
        assert!(tree_may_have_named_process(
            10,
            &entries,
            |pid| { Some(u64::from(pid)) },
            "opencode"
        ));
        assert!(!tree_may_have_named_process(
            10,
            &entries,
            |pid| { Some(u64::from(pid)) },
            "codex"
        ));
        assert!(tree_may_have_named_process(10, &[], |_| None, "opencode"));
        assert!(tree_may_have_named_process(
            10,
            &entries,
            |pid| { (pid != 20).then_some(u64::from(pid)) },
            "opencode"
        ));
        assert!(!tree_may_have_named_process(
            10,
            &entries,
            |pid| Some(u64::from(pid)),
            "codex"
        ));
    }

    #[test]
    fn process_without_a_start_time_is_excluded_with_its_subtree() {
        let entries = [entry(20, 10), entry(30, 20)];
        let starts = |pid: u32| (pid != 20).then_some(u64::from(pid));
        assert_eq!(pids(&launch_tree(10, &entries, starts)), vec![(10, 0)]);
    }

    #[test]
    fn parent_links_that_cycle_terminate() {
        let entries = [entry(10, 20), entry(20, 10)];
        let tree = launch_tree(10, &entries, |_| Some(1));
        assert_eq!(pids(&tree), vec![(10, 0), (20, 1)]);
    }

    #[test]
    fn unknown_root_yields_no_tree() {
        assert!(launch_tree(10, &[entry(20, 10)], |_| None).is_empty());
    }
}
