use super::{executable, failure, native, process_commands, shell_command, Boundary};
use crate::service::agent_runtime::model::RuntimeFailure;
use codeterm_plugin::agent_runtime::MAX_BACKGROUND_PROCESSES;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) struct HostMcpFilter {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) struct ObservedProcess {
    pub pid: u32,
    pub parent_pid: u32,
    pub born: u128,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
}

const MAX_BACKGROUND_CHILDREN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) struct SelectedBackgroundChild {
    pub pid: u32,
    pub born: u128,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) struct SelectedBackgroundProcess {
    pub pid: u32,
    pub born: u128,
    pub parent_pid: Option<u32>,
    pub command: String,
    pub cwd: String,
    pub children: Vec<SelectedBackgroundChild>,
}

pub(in crate::service::agent_runtime) struct OsBackgroundSnapshot {
    pub entries: Vec<SelectedBackgroundProcess>,
    pub watch_handles: Vec<(u32, u128, native::Member)>,
}

pub(in crate::service::agent_runtime) fn format_os_process_id(pid: u32, born: u128) -> String {
    format!("os:{pid}:{born}")
}

pub(in crate::service::agent_runtime) fn parse_os_process_id(
    process_id: &str,
) -> Option<(u32, u128)> {
    let rest = process_id.strip_prefix("os:")?;
    let (pid, born) = rest.split_once(':')?;
    let pid = pid.parse::<u32>().ok()?;
    let born = born.parse::<u128>().ok()?;
    (pid > 1).then_some((pid, born))
}

fn is_console_host(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    matches!(
        executable(first).to_ascii_lowercase().as_str(),
        "conhost" | "openconsole" | "csrss"
    )
}

fn is_host_mcp(argv: &[String], filter: Option<&HostMcpFilter>) -> bool {
    let Some(filter) = filter else {
        return false;
    };
    let Some(first) = argv.first() else {
        return false;
    };
    if !executable(first).eq_ignore_ascii_case(&executable(&filter.command)) {
        return false;
    }
    if filter.args.is_empty() {
        return false;
    }
    argv.len() > filter.args.len() && argv[1..=filter.args.len()] == filter.args[..]
}

fn looks_like_mcp_token(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let leaf = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    let stem = leaf.strip_suffix(".exe").unwrap_or(leaf);
    stem == "mcp-server"
        || stem == "mcp_server"
        || stem.ends_with("-mcp")
        || stem.ends_with("_mcp")
        || stem.ends_with("-mcp-server")
        || stem.ends_with("_mcp_server")
        || lower.contains("@modelcontextprotocol/")
}

fn is_persistent_mcp_sidecar(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    if shell_command(argv).is_some() {
        return false;
    }
    if looks_like_mcp_token(first) {
        return true;
    }
    let runner = executable(first).to_ascii_lowercase();
    matches!(
        runner.as_str(),
        "node"
            | "npx"
            | "npm"
            | "pnpm"
            | "bun"
            | "bunx"
            | "deno"
            | "python"
            | "python3"
            | "uv"
            | "uvx"
    ) && argv
        .iter()
        .skip(1)
        .take(4)
        .any(|arg| !arg.starts_with('-') && looks_like_mcp_token(arg))
}

fn is_compiler_daemon(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    if shell_command(argv).is_some() {
        return false;
    }
    matches!(
        executable(first).to_ascii_lowercase().as_str(),
        "sccache" | "sccache-dist" | "ccache" | "buildcache"
    )
}

fn is_plumbing_helper(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    is_bare_shell(argv)
        || is_compiler_daemon(argv)
        || matches!(
            executable(first).to_ascii_lowercase().as_str(),
            "tee" | "cat" | "head" | "tail"
        )
}

fn is_bare_shell(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    if shell_command(argv).is_some() {
        return false;
    }
    let name = executable(first).to_ascii_lowercase();
    match name.as_str() {
        "sh" | "bash" | "zsh" | "dash" => argv.iter().skip(1).all(|arg| {
            matches!(
                arg.as_str(),
                "-l" | "-i" | "--login" | "--noprofile" | "--norc"
            )
        }),
        "pwsh" | "powershell" => argv.iter().skip(1).all(|arg| {
            let lower = arg.to_ascii_lowercase();
            matches!(
                lower.as_str(),
                "-nologo" | "-noprofile" | "-noexit" | "-noninteractive" | "-sta" | "-mta"
            )
        }),
        "cmd" => argv.iter().skip(1).all(|arg| {
            let lower = arg.to_ascii_lowercase();
            matches!(lower.as_str(), "/q" | "/d" | "/u" | "/a" | "/k")
        }),
        _ => false,
    }
}

fn format_argv(argv: &[String]) -> String {
    let mut parts = Vec::with_capacity(argv.len());
    for (index, arg) in argv.iter().enumerate() {
        let clean = arg.replace('\0', "");
        if index == 0 {
            let exe = executable(&clean);
            if !exe.is_empty() {
                parts.push(exe);
                continue;
            }
        }
        if clean.is_empty() || clean.chars().any(char::is_whitespace) {
            parts.push(format!("\"{}\"", clean.replace('"', "\\\"")));
        } else {
            parts.push(clean);
        }
    }
    parts.join(" ")
}

fn bounded_command(raw: &str) -> String {
    let clean = raw.replace('\0', "");
    let trimmed = clean.trim();
    if trimmed.len() <= 65536 {
        return trimmed.to_owned();
    }
    let mut end = 65536;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_owned()
}

fn sanitize_cwd(cwd: Option<&PathBuf>, fallback: &str) -> String {
    let candidate = cwd
        .and_then(|path| path.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 4096 && !value.contains('\0'));
    if let Some(value) = candidate {
        return codeterm_paths::strip_verbatim_prefix(value);
    }
    let fallback = fallback.trim();
    if !fallback.is_empty() && fallback.len() <= 4096 && !fallback.contains('\0') {
        fallback.to_owned()
    } else {
        ".".into()
    }
}

fn command_for_process(argv: &[String]) -> Option<String> {
    if argv.is_empty() {
        return None;
    }
    if let Some(script) = shell_command(argv) {
        let trimmed = script.trim();
        if !trimmed.is_empty() {
            return Some(bounded_command(trimmed));
        }
    }
    let formatted = bounded_command(&format_argv(argv));
    (!formatted.is_empty()).then_some(formatted)
}

pub(in crate::service::agent_runtime) fn select_background_processes(
    root_pid: u32,
    observed: &[ObservedProcess],
    baseline: &[(u32, u128)],
    host_mcp: Option<&HostMcpFilter>,
    fallback_cwd: &str,
) -> Vec<SelectedBackgroundProcess> {
    let baseline_set: HashSet<(u32, u128)> = baseline.iter().copied().collect();
    let mut excluded_sidecars: HashSet<u32> = HashSet::new();
    for process in observed {
        if is_host_mcp(&process.argv, host_mcp) || is_persistent_mcp_sidecar(&process.argv) {
            excluded_sidecars.insert(process.pid);
        }
    }
    loop {
        let before = excluded_sidecars.len();
        for process in observed {
            if excluded_sidecars.contains(&process.parent_pid) {
                excluded_sidecars.insert(process.pid);
            }
        }
        if excluded_sidecars.len() == before {
            break;
        }
    }

    let candidates: Vec<&ObservedProcess> = observed
        .iter()
        .filter(|process| {
            process.pid > 1
                && process.pid != root_pid
                && !baseline_set.contains(&(process.pid, process.born))
                && !excluded_sidecars.contains(&process.pid)
                && !is_console_host(&process.argv)
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let baseline_pids: HashSet<u32> = baseline.iter().map(|(pid, _)| *pid).collect();
    let observed_by_pid: HashMap<u32, &ObservedProcess> = observed
        .iter()
        .map(|process| (process.pid, process))
        .collect();
    let candidate_pids: HashSet<u32> = candidates.iter().map(|process| process.pid).collect();

    // Walk the full observed parent chain so excluded wrappers (e.g. conhost) never split a tree.
    let mut root_for_candidate: HashMap<u32, u32> = HashMap::with_capacity(candidates.len());
    for process in &candidates {
        let mut cursor = process.parent_pid;
        let mut top_candidate = process.pid;
        let mut visited = HashSet::from([process.pid]);
        for _ in 0..64 {
            if cursor <= 1 || cursor == root_pid || !visited.insert(cursor) {
                break;
            }
            if candidate_pids.contains(&cursor) {
                top_candidate = cursor;
            }
            let Some(parent) = observed_by_pid.get(&cursor) else {
                break;
            };
            cursor = parent.parent_pid;
        }
        root_for_candidate.insert(process.pid, top_candidate);
    }

    let mut descendants_by_root: HashMap<u32, Vec<&ObservedProcess>> = HashMap::new();
    let mut raw_roots: Vec<&ObservedProcess> = Vec::new();
    for process in &candidates {
        let top = root_for_candidate[&process.pid];
        if top == process.pid {
            raw_roots.push(*process);
        } else {
            descendants_by_root.entry(top).or_default().push(*process);
        }
    }
    for descendants in descendants_by_root.values_mut() {
        descendants.sort_by_key(|child| (child.born, child.pid));
    }
    raw_roots.sort_by_key(|process| (process.born, process.pid));

    // Group sibling roots that share the same exited launcher shell (e.g. `cmd | tee`).
    let exited_launcher_parent = |process: &ObservedProcess| -> Option<u32> {
        (process.parent_pid > 1
            && process.parent_pid != root_pid
            && !baseline_pids.contains(&process.parent_pid)
            && !observed_by_pid.contains_key(&process.parent_pid))
        .then_some(process.parent_pid)
    };
    struct RootGroup<'a> {
        members: Vec<&'a ObservedProcess>,
        dead_parent: Option<u32>,
    }
    let mut groups: Vec<RootGroup<'_>> = Vec::new();
    let mut group_by_dead_parent: HashMap<u32, usize> = HashMap::new();
    for root in raw_roots {
        if let Some(dead_parent) = exited_launcher_parent(root) {
            if let Some(&index) = group_by_dead_parent.get(&dead_parent) {
                groups[index].members.push(root);
            } else {
                let index = groups.len();
                group_by_dead_parent.insert(dead_parent, index);
                groups.push(RootGroup {
                    members: vec![root],
                    dead_parent: Some(dead_parent),
                });
            }
        } else {
            groups.push(RootGroup {
                members: vec![root],
                dead_parent: None,
            });
        }
    }
    for group in &mut groups {
        group
            .members
            .sort_by_key(|member| (is_plumbing_helper(&member.argv), member.born, member.pid));
    }

    struct ActiveGroup<'a> {
        group: RootGroup<'a>,
        command: String,
        cwd: String,
        promoted_child_pid: Option<u32>,
    }
    let mut active_groups: Vec<ActiveGroup<'_>> = Vec::new();
    let mut orphan_daemons: Vec<&ObservedProcess> = Vec::new();
    for group in groups {
        let lead = group.members[0];
        let is_orphan_daemon = group.dead_parent.is_some()
            && group.members.len() == 1
            && is_compiler_daemon(&lead.argv)
            && descendants_by_root.get(&lead.pid).is_none_or(Vec::is_empty);
        if is_orphan_daemon {
            orphan_daemons.push(lead);
            continue;
        }
        let mut promoted_child_pid = None;
        let command = if is_bare_shell(&lead.argv) {
            let promoted = descendants_by_root.get(&lead.pid).and_then(|children| {
                children
                    .iter()
                    .filter_map(|child| command_for_process(&child.argv).map(|cmd| (child, cmd)))
                    .min_by_key(|(child, _)| {
                        (is_plumbing_helper(&child.argv), child.born, child.pid)
                    })
            });
            if let Some((child, cmd)) = promoted {
                promoted_child_pid = Some(child.pid);
                cmd
            } else if let Some(cmd) = group
                .members
                .iter()
                .skip(1)
                .find_map(|member| command_for_process(&member.argv))
            {
                cmd
            } else {
                continue;
            }
        } else {
            let Some(cmd) = command_for_process(&lead.argv) else {
                continue;
            };
            cmd
        };
        let cwd = sanitize_cwd(lead.cwd.as_ref(), fallback_cwd);
        active_groups.push(ActiveGroup {
            group,
            command,
            cwd,
            promoted_child_pid,
        });
    }

    // Attach detached compiler daemons (e.g. sccache) to the running command that spawned them;
    // if no other command is running, keep the daemon visible so live work is never hidden.
    let mut attached_daemons: Vec<Vec<&ObservedProcess>> = vec![Vec::new(); active_groups.len()];
    let mut standalone_daemons: Vec<&ObservedProcess> = Vec::new();
    for daemon in orphan_daemons {
        if active_groups.is_empty() {
            standalone_daemons.push(daemon);
            continue;
        }
        let daemon_cwd = sanitize_cwd(daemon.cwd.as_ref(), fallback_cwd);
        let target = active_groups
            .iter()
            .enumerate()
            .filter(|(_, active)| {
                active.cwd == daemon_cwd && active.group.members[0].born <= daemon.born
            })
            .max_by_key(|(_, active)| active.group.members[0].born)
            .or_else(|| {
                active_groups
                    .iter()
                    .enumerate()
                    .filter(|(_, active)| active.group.members[0].born <= daemon.born)
                    .max_by_key(|(_, active)| active.group.members[0].born)
            })
            .or_else(|| (active_groups.len() == 1).then(|| (0, &active_groups[0])))
            .map(|(index, _)| index);
        if let Some(index) = target {
            attached_daemons[index].push(daemon);
        } else {
            standalone_daemons.push(daemon);
        }
    }

    let mut selected = Vec::new();
    for (active, extra_daemons) in active_groups.into_iter().zip(attached_daemons) {
        if selected.len() >= MAX_BACKGROUND_PROCESSES {
            break;
        }
        let ActiveGroup {
            group,
            command,
            cwd,
            promoted_child_pid,
        } = active;
        let lead = group.members[0];
        let mut child_nodes: Vec<&ObservedProcess> = Vec::new();
        let mut seen_children = HashSet::from([lead.pid]);
        if let Some(promoted) = promoted_child_pid {
            seen_children.insert(promoted);
        }
        for member in &group.members {
            if seen_children.insert(member.pid) {
                child_nodes.push(*member);
            }
            if let Some(descendants) = descendants_by_root.get(&member.pid) {
                for descendant in descendants {
                    if seen_children.insert(descendant.pid) {
                        child_nodes.push(*descendant);
                    }
                }
            }
        }
        for daemon in extra_daemons {
            if seen_children.insert(daemon.pid) {
                child_nodes.push(daemon);
            }
        }
        child_nodes.sort_by_key(|child| (child.born, child.pid));
        let children = child_nodes
            .into_iter()
            .filter_map(|child| {
                command_for_process(&child.argv).map(|cmd| SelectedBackgroundChild {
                    pid: child.pid,
                    born: child.born,
                    command: cmd,
                })
            })
            .take(MAX_BACKGROUND_CHILDREN)
            .collect();
        selected.push(SelectedBackgroundProcess {
            pid: lead.pid,
            born: lead.born,
            parent_pid: group.dead_parent,
            command,
            cwd,
            children,
        });
    }
    for daemon in standalone_daemons {
        if selected.len() >= MAX_BACKGROUND_PROCESSES {
            break;
        }
        let Some(command) = command_for_process(&daemon.argv) else {
            continue;
        };
        selected.push(SelectedBackgroundProcess {
            pid: daemon.pid,
            born: daemon.born,
            parent_pid: exited_launcher_parent(daemon),
            command,
            cwd: sanitize_cwd(daemon.cwd.as_ref(), fallback_cwd),
            children: Vec::new(),
        });
    }
    selected
}

#[cfg(windows)]
pub(in crate::service::agent_runtime) struct ExitWaitCancel {
    handle: usize,
}

#[cfg(windows)]
impl ExitWaitCancel {
    pub(in crate::service::agent_runtime) fn new() -> Result<Self, String> {
        use windows_sys::Win32::System::Threading::CreateEventW;
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            handle: handle as usize,
        })
    }

    pub(in crate::service::agent_runtime) fn cancel(&self) {
        use windows_sys::Win32::{Foundation::HANDLE, System::Threading::SetEvent};
        unsafe {
            SetEvent(self.handle as HANDLE);
        }
    }

    fn raw_handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.handle as windows_sys::Win32::Foundation::HANDLE
    }
}

#[cfg(windows)]
impl Drop for ExitWaitCancel {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}

#[cfg(unix)]
pub(in crate::service::agent_runtime) struct ExitWaitCancel {
    read_fd: std::os::fd::OwnedFd,
    write_fd: parking_lot::Mutex<Option<std::os::fd::OwnedFd>>,
}

#[cfg(unix)]
impl ExitWaitCancel {
    pub(in crate::service::agent_runtime) fn new() -> Result<Self, String> {
        use std::os::fd::FromRawFd;
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let read_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fds[0]) };
        let write_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fds[1]) };
        for fd in [fds[0], fds[1]] {
            let _ = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags >= 0 {
                let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
            }
        }
        Ok(Self {
            read_fd,
            write_fd: parking_lot::Mutex::new(Some(write_fd)),
        })
    }

    pub(in crate::service::agent_runtime) fn cancel(&self) {
        use std::os::fd::AsRawFd;
        if let Some(writer) = self.write_fd.lock().take() {
            let byte = [1u8];
            let _ = unsafe { libc::write(writer.as_raw_fd(), byte.as_ptr().cast(), 1) };
        }
    }

    fn read_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.read_fd.as_raw_fd()
    }
}

pub(in crate::service::agent_runtime) fn wait_for_any_exit(
    handles: Vec<native::Member>,
    cancel: Arc<ExitWaitCancel>,
) {
    if handles.is_empty() {
        return;
    }
    const MAX_WAIT_PROCESSES: usize = 63;
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::HANDLE,
            System::Threading::{WaitForMultipleObjects, INFINITE},
        };
        let mut wait_handles: Vec<HANDLE> =
            Vec::with_capacity(handles.len().min(MAX_WAIT_PROCESSES) + 1);
        wait_handles.push(cancel.raw_handle());
        for member in handles.iter().take(MAX_WAIT_PROCESSES) {
            wait_handles.push(member.raw_handle());
        }
        unsafe {
            WaitForMultipleObjects(
                wait_handles.len() as u32,
                wait_handles.as_ptr(),
                0,
                INFINITE,
            );
        }
    }
    #[cfg(unix)]
    {
        let mut pollfds: Vec<libc::pollfd> =
            Vec::with_capacity(handles.len().min(MAX_WAIT_PROCESSES) + 1);
        pollfds.push(libc::pollfd {
            fd: cancel.read_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        for member in handles.iter().take(MAX_WAIT_PROCESSES) {
            pollfds.push(libc::pollfd {
                fd: member.event_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        loop {
            let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1) };
            if rc < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
    }
}

impl Boundary {
    pub(in crate::service::agent_runtime) fn baseline_identities(
        &self,
    ) -> Result<Vec<(u32, u128)>, RuntimeFailure> {
        let mut ids = HashSet::new();
        #[cfg(windows)]
        {
            for descendant in native::tree(self).map_err(failure)? {
                if !descendant.member.exited().unwrap_or(true) {
                    ids.insert((descendant.pid, descendant.member.token()));
                }
            }
            if let Ok(job_pids) = native::ids(self) {
                for pid in job_pids {
                    if pid == self.pid || ids.len() >= 256 {
                        continue;
                    }
                    if let Ok(Some(member)) = native::Member::open(self, pid) {
                        if !member.exited().unwrap_or(true) {
                            ids.insert((pid, member.token()));
                        }
                    }
                }
            }
        }
        #[cfg(unix)]
        {
            for member in self.snapshot()? {
                if !member.native.exited().unwrap_or(true) {
                    ids.insert((member.pid, member.native.token()));
                }
            }
        }
        Ok(ids.into_iter().collect())
    }

    pub(in crate::service::agent_runtime) fn observe_background(
        &self,
        baseline: &[(u32, u128)],
        host_mcp: Option<&HostMcpFilter>,
        fallback_cwd: &str,
    ) -> Result<OsBackgroundSnapshot, RuntimeFailure> {
        #[cfg(windows)]
        let (observed, mut members) = {
            let descendants = native::tree(self).map_err(failure)?;
            let mut members: HashMap<u32, (u32, native::Member)> = HashMap::new();
            for descendant in descendants {
                members.insert(descendant.pid, (descendant.parent, descendant.member));
            }
            if let Ok(job_pids) = native::ids(self) {
                let parent_map = native::parents().unwrap_or_default();
                for pid in job_pids {
                    if pid == self.pid || members.contains_key(&pid) || members.len() >= 256 {
                        continue;
                    }
                    if let Ok(Some(member)) = native::Member::open(self, pid) {
                        let parent = parent_map.get(&pid).copied().unwrap_or(self.pid);
                        members.insert(pid, (parent, member));
                    }
                }
            }
            let system = process_commands(members.keys().copied());
            let mut observed = Vec::with_capacity(members.len());
            for (&pid, (parent_pid, member)) in &members {
                if self.infrastructure_process(pid, member) || member.exited().unwrap_or(true) {
                    continue;
                }
                let (argv, cwd) = super::process_command(&system, pid).unwrap_or_default();
                observed.push(ObservedProcess {
                    pid,
                    parent_pid: *parent_pid,
                    born: member.token(),
                    argv,
                    cwd,
                });
            }
            (observed, members)
        };

        #[cfg(unix)]
        let (observed, mut members) = {
            let ids = native::ids(self).map_err(failure)?;
            if ids.len() > 256 {
                return Err(failure("process snapshot exceeds 256 members"));
            }
            let system = process_commands(ids.iter().copied());
            let mut members: HashMap<u32, (u32, native::Member)> = HashMap::new();
            let mut observed = Vec::new();
            for pid in ids {
                if pid == self.pid {
                    continue;
                }
                let Some(member) = native::Member::open(self, pid).map_err(failure)? else {
                    continue;
                };
                if self.infrastructure_process(pid, &member) || member.exited().unwrap_or(true) {
                    continue;
                }
                let (argv, cwd, parent_pid) =
                    super::process_command_with_parent(&system, pid).unwrap_or_default();
                observed.push(ObservedProcess {
                    pid,
                    parent_pid,
                    born: member.token(),
                    argv,
                    cwd,
                });
                members.insert(pid, (parent_pid, member));
            }
            (observed, members)
        };

        let entries =
            select_background_processes(self.pid, &observed, baseline, host_mcp, fallback_cwd);
        let mut watch_handles = Vec::with_capacity(entries.len());
        for entry in &entries {
            if let Some((_, member)) = members.remove(&entry.pid) {
                if member.token() == entry.born {
                    watch_handles.push((entry.pid, entry.born, member));
                }
            }
            for child in &entry.children {
                if let Some((_, member)) = members.remove(&child.pid) {
                    if member.token() == child.born {
                        watch_handles.push((child.pid, child.born, member));
                    }
                }
            }
        }
        Ok(OsBackgroundSnapshot {
            entries,
            watch_handles,
        })
    }

    pub(in crate::service::agent_runtime) fn stop_os_background(
        &self,
        targets: &[(u32, u128)],
        baseline: &[(u32, u128)],
        others: &[(u32, u128)],
    ) -> Result<(), RuntimeFailure> {
        if targets.is_empty() {
            return Ok(());
        }
        for &(target_pid, target_born) in targets {
            if target_pid == self.pid
                || baseline
                    .iter()
                    .any(|&(pid, born)| pid == target_pid && born == target_born)
            {
                return Err(failure("Cannot stop runtime infrastructure process"));
            }
        }

        #[cfg(windows)]
        {
            let tree = self.capture_tools()?;
            let mut nodes = Vec::with_capacity(tree.nodes.len());
            let mut candidate_positions: Vec<(usize, (u32, u128))> = Vec::new();
            let mut protected = HashSet::new();
            for (position, node) in tree.nodes.iter().enumerate() {
                let exited = node.member.exited().map_err(failure)?;
                let infra = self.infrastructure_process(node.pid, &node.member)
                    || baseline
                        .iter()
                        .any(|&(pid, born)| pid == node.pid && born == node.member.token());
                if !exited
                    && !infra
                    && targets
                        .iter()
                        .any(|&(pid, born)| pid == node.pid && born == node.member.token())
                {
                    candidate_positions.push((position, (node.pid, node.member.token())));
                }
                if !exited
                    && others
                        .iter()
                        .any(|&(pid, born)| pid == node.pid && born == node.member.token())
                {
                    protected.insert(position);
                }
                nodes.push(super::orphans::Node {
                    pid: node.pid,
                    parent: node.parent,
                    infrastructure: infra,
                    exited,
                });
            }
            let mut handled: HashSet<(u32, u128)> = HashSet::new();
            let mut branch: HashSet<usize> = HashSet::new();
            for (position, identity) in candidate_positions {
                if branch.contains(&position) {
                    handled.insert(identity);
                    continue;
                }
                let Some(sub) = super::selective::subtree(&nodes, &[position], &protected) else {
                    return Err(failure(
                        "Target background process subtree could not be isolated",
                    ));
                };
                branch.extend(sub);
                handled.insert(identity);
            }
            if !branch.is_empty() {
                let nodes = tree
                    .nodes
                    .into_iter()
                    .enumerate()
                    .filter_map(|(position, node)| branch.contains(&position).then_some(node))
                    .collect();
                self.reap_background(super::BackgroundTree {
                    tree: super::ToolTree { nodes },
                })?;
            }
            for &(target_pid, target_born) in targets {
                if handled.contains(&(target_pid, target_born)) {
                    continue;
                }
                let Some(member) = native::Member::open(self, target_pid).map_err(failure)? else {
                    continue;
                };
                if member.exited().map_err(failure)? {
                    continue;
                }
                if member.token() != target_born || self.infrastructure_process(target_pid, &member)
                {
                    return Err(failure("Background process identity changed"));
                }
                member.terminate(self).map_err(failure)?;
                member
                    .wait(std::time::Instant::now() + std::time::Duration::from_secs(5))
                    .map_err(failure)?;
            }
            Ok(())
        }

        #[cfg(unix)]
        {
            let _ = others;
            for &(target_pid, target_born) in targets {
                let Some(member) = native::Member::open(self, target_pid).map_err(failure)? else {
                    continue;
                };
                if member.exited().map_err(failure)? {
                    continue;
                }
                if member.token() != target_born || self.infrastructure_process(target_pid, &member)
                {
                    return Err(failure("Background process identity changed"));
                }
                member.terminate(self).map_err(failure)?;
                member
                    .wait(std::time::Instant::now() + std::time::Duration::from_secs(5))
                    .map_err(failure)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
