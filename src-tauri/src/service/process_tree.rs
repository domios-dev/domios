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
    argv0_basename: Option<String>,
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
    entries
        .iter()
        .any(|entry| pids.contains(&entry.pid) && process_name_matches(entry, expected_name))
}

pub(crate) struct ProviderTreeWitnesses {
    observations: Mutex<HashMap<ProviderTreeKey, ProviderTreeHistory>>,
    #[cfg(test)]
    scripted: Mutex<HashMap<ProviderTreeKey, VecDeque<ProviderProcessTreeSnapshot>>>,
    #[cfg(test)]
    observation_senders:
        Mutex<HashMap<ProviderTreeKey, std::sync::mpsc::Sender<ProviderTreeObservation>>>,
}

impl Default for ProviderTreeWitnesses {
    fn default() -> Self {
        Self {
            observations: Mutex::new(HashMap::new()),
            #[cfg(test)]
            scripted: Mutex::new(HashMap::new()),
            #[cfg(test)]
            observation_senders: Mutex::new(HashMap::new()),
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

    #[cfg(test)]
    pub(crate) fn report_next_observation(
        &self,
        key: &ProviderTreeKey,
        sender: std::sync::mpsc::Sender<ProviderTreeObservation>,
    ) {
        self.observation_senders.lock().insert(key.clone(), sender);
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
        let observation = ProviderTreeObservation {
            key: key.clone(),
            state,
            provider_seen_before,
            consecutive_absent_observations: history.consecutive_absent_observations,
        };
        drop(observations);
        #[cfg(test)]
        if let Some(sender) = self.observation_senders.lock().remove(key) {
            let _ = sender.send(observation.clone());
        }
        observation
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
                .is_some_and(|entry| process_name_matches(entry, expected_name))
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
            && process_name_matches(entry, expected_name)
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

fn process_name_matches(entry: &ProcessEntry, expected: &str) -> bool {
    executable_names_match(&entry.image_name, expected)
        || entry
            .argv0_basename
            .as_deref()
            .is_some_and(|name| executable_names_match(name, expected))
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod argv0;

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
                argv0_basename: None,
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
                    argv0_basename: std::fs::read(format!("/proc/{pid}/cmdline"))
                        .ok()
                        .and_then(|bytes| super::argv0::from_cmdline(&bytes)),
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
        let mut args = vec![0; argument_capacity().unwrap_or(0)];
        pids.into_iter()
            .filter(|pid| *pid > 0)
            .filter_map(|pid| {
                let info = bsd_info(pid as u32)?;
                Some(ProcessEntry {
                    pid: pid as u32,
                    parent_pid: info.pbi_ppid,
                    argv0_basename: argv0_basename(pid, &mut args),
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

    fn argument_capacity() -> Option<usize> {
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        let mut capacity: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&capacity);
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as _,
                (&mut capacity as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (result == 0 && size == std::mem::size_of_val(&capacity) && capacity > 0)
            .then_some(capacity as usize)
    }

    fn argv0_basename(pid: libc::pid_t, args: &mut [u8]) -> Option<String> {
        if args.is_empty() {
            return None;
        }
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size = args.len();
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as _,
                args.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return None;
        }
        super::argv0::from_procargs(args.get(..size)?)
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
mod tests;
