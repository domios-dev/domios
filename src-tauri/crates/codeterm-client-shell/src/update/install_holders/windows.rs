use super::model::{
    classify_holder, foreign_notice, install_binaries, is_owned_server_holder,
    same_process_identity, Holder, HolderDisposition, ProcessRecord, INSTALL_BINARY_DIRS,
};
use codeterm_util::process_tree::{process_table, OwnedProcessTree};
use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, FILETIME, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::RestartManager::*;
use windows_sys::Win32::System::Threading::*;

struct Handle(HANDLE);
// Kernel process handles may be waited on and closed from another thread.
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
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
            "start install-holder discovery",
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
            "register install files",
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
                return rm_result(result, "enumerate install-file holders").map(|()| holders);
            }
            holders.resize(needed as usize, RM_PROCESS_INFO::default());
        }
    }
}

fn holds_registered_file(info: &RM_PROCESS_INFO, files: &[PathBuf]) -> Result<bool, String> {
    let process = &info.Process;
    for file in files {
        let session = Session::register(std::slice::from_ref(file))?;
        if session.holders()?.iter().any(|candidate| {
            candidate.Process.dwProcessId == process.dwProcessId
                && same_process_identity(
                    (
                        candidate.Process.ProcessStartTime.dwLowDateTime,
                        candidate.Process.ProcessStartTime.dwHighDateTime,
                    ),
                    (
                        process.ProcessStartTime.dwLowDateTime,
                        process.ProcessStartTime.dwHighDateTime,
                    ),
                )
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

struct PinnedHolder {
    holder: Holder,
    handle: Option<Handle>,
    disposition: HolderDisposition,
}

pub(super) struct OwnedServerHolders {
    holders: Vec<PinnedHolder>,
    tree: OwnedProcessTree,
}

impl OwnedServerHolders {
    pub(super) fn confirm_exited(self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        self.tree
            .terminate_remaining(std::time::Duration::from_secs(30))
            .map_err(|error| error.to_string())?;
        for holder in self.holders {
            let handle = holder
                .handle
                .ok_or("owned server holder has no exit handle")?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if unsafe { WaitForSingleObject(handle.0, remaining.as_millis() as u32) }
                != WAIT_OBJECT_0
            {
                return Err(format!(
                    "{} (PID {}) did not exit with the attached server; installer launch cancelled",
                    holder.holder.name, holder.holder.pid
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn preflight(
    server: crate::application_host::AttachedServerIdentity,
) -> Result<OwnedServerHolders, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let root = executable.parent().ok_or("install directory unresolved")?;
    inspect_directory_with_server(root, false, Some(&server))
}

pub(super) fn inspect(release: bool) -> Result<(), String> {
    let executable =
        std::fs::canonicalize(std::env::current_exe().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let root = executable.parent().ok_or("install directory unresolved")?;
    inspect_directory(root, release)
}

fn inspect_directory(root: &Path, release: bool) -> Result<(), String> {
    inspect_directory_with_server(root, release, None).map(|_| ())
}

fn inspect_directory_with_server(
    root: &Path,
    release: bool,
    server: Option<&crate::application_host::AttachedServerIdentity>,
) -> Result<OwnedServerHolders, String> {
    let files = collect_install_binaries(root)?;
    let mut clis = files
        .iter()
        .filter(|path| {
            path.parent() == Some(root.join("cli").as_path())
                && path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy().to_ascii_lowercase();
                    [
                        "codeterm.exe",
                        "codeterm-x86_64-pc-windows-msvc.exe",
                        "codeterm-aarch64-pc-windows-msvc.exe",
                        "codeterm-i686-pc-windows-msvc.exe",
                    ]
                    .contains(&name.as_str())
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    clis.push(codeterm_paths::data_dir().join("bin/codeterm.exe"));
    let helpers = [
        root.join("conpty/x64/OpenConsole.exe"),
        root.join("conpty/arm64/OpenConsole.exe"),
    ];
    let mut server_images = helpers.to_vec();
    server_images.push(root.join("domios-server/domios-server.exe"));
    let session = Session::register(&files)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let processes = process_table().map_err(|error| error.to_string())?;
        let mut pinned = Vec::new();
        for info in session.holders()? {
            let created = ((info.Process.ProcessStartTime.dwHighDateTime as u64) << 32)
                | info.Process.ProcessStartTime.dwLowDateTime as u64;
            if super::model::is_attached_server(server, info.Process.dwProcessId, created)
                || info.Process.dwProcessId == std::process::id()
                || !holds_registered_file(&info, &files)?
            {
                continue;
            }
            if let Some(mut holder) = pin_holder(&info, &processes, &clis, &helpers)? {
                if server.is_some_and(|server| {
                    is_owned_server_holder(
                        server,
                        holder.holder.pid,
                        created,
                        holder.holder.executable.as_deref(),
                        &server_images,
                        &processes,
                    )
                }) {
                    holder.disposition = HolderDisposition::OwnedServer;
                }
                pinned.push(holder);
            }
        }
        let mut foreign = Vec::new();
        let mut owned = Vec::new();
        let mut owned_server = Vec::new();
        for holder in pinned {
            match holder.disposition {
                HolderDisposition::Foreign => foreign.push(holder.holder),
                HolderDisposition::OwnedHelper => owned.push(holder),
                HolderDisposition::OwnedServer => owned_server.push(holder),
                HolderDisposition::OwnedTab => {
                    if release {
                        return Err(format!(
                            "{} still holds install files",
                            holder.holder.owned_tab.as_deref().unwrap_or("Owned Tab")
                        ));
                    }
                }
                HolderDisposition::UpdatingApp => {}
            }
        }
        if !foreign.is_empty() {
            return Err(foreign_notice(&foreign));
        }
        if !release || owned.is_empty() {
            let tree = match server {
                Some(server) => OwnedProcessTree::capture(
                    (
                        server.pid,
                        server
                            .created
                            .parse()
                            .map_err(|_| "invalid attached server creation time")?,
                    ),
                    &server_images,
                )
                .map_err(|error| error.to_string())?,
                None => OwnedProcessTree::default(),
            };
            return Ok(OwnedServerHolders {
                holders: owned_server,
                tree,
            });
        }
        for holder in &owned {
            terminate(holder)?;
        }
        for holder in owned {
            let handle = holder
                .handle
                .as_ref()
                .ok_or("owned install holder has no exit handle")?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if unsafe { WaitForSingleObject(handle.0, remaining.as_millis() as u32) }
                != WAIT_OBJECT_0
            {
                return Err(last_error(&format!(
                    "waiting for {} (PID {})",
                    holder.holder.name, holder.holder.pid
                )));
            }
            log::debug!(
                "[updater] released install holder {} (PID {})",
                holder.holder.name,
                holder.holder.pid
            );
        }
        if std::time::Instant::now() >= deadline {
            return Err("install-holder release deadline".into());
        }
    }
}

fn pin_holder(
    info: &RM_PROCESS_INFO,
    processes: &HashMap<u32, ProcessRecord>,
    clis: &[PathBuf],
    helpers: &[PathBuf],
) -> Result<Option<PinnedHolder>, String> {
    let pid = info.Process.dwProcessId;
    let mut holder = Holder {
        pid,
        name: wide_name(&info.strAppName),
        executable: None,
        parent_pid: processes.get(&pid).map(|process| process.parent_pid),
        owned_tab: None,
    };
    if holder.name.is_empty() {
        holder.name = "Unknown process".into();
    }
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    if raw.is_null() {
        if std::io::Error::last_os_error().raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(None);
        }
        return Ok(Some(PinnedHolder {
            holder,
            handle: None,
            disposition: HolderDisposition::Foreign,
        }));
    }
    let handle = Handle(raw);
    if unsafe { WaitForSingleObject(raw, 0) } == WAIT_OBJECT_0 {
        return Ok(None);
    }
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(raw, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Ok(Some(PinnedHolder {
            holder,
            handle: Some(handle),
            disposition: HolderDisposition::Foreign,
        }));
    }
    let expected = info.Process.ProcessStartTime;
    if !same_process_identity(
        (expected.dwLowDateTime, expected.dwHighDateTime),
        (creation.dwLowDateTime, creation.dwHighDateTime),
    ) {
        return Ok(None);
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(raw, 0, path.as_mut_ptr(), &mut length) } != 0 {
        let path = PathBuf::from(String::from_utf16_lossy(&path[..length as usize]));
        if let Some(name) = path.file_name() {
            holder.name = name.to_string_lossy().into_owned();
        }
        holder.executable = Some(path);
    }
    let disposition = classify_holder(&holder, std::process::id(), clis, helpers);
    Ok(Some(PinnedHolder {
        holder,
        handle: Some(handle),
        disposition,
    }))
}

fn terminate(holder: &PinnedHolder) -> Result<(), String> {
    let process = holder
        .handle
        .as_ref()
        .ok_or("owned holder has no pinned identity")?;
    if unsafe { TerminateProcess(process.0, 1) } == 0
        && unsafe { WaitForSingleObject(process.0, 0) } != WAIT_OBJECT_0
    {
        return Err(last_error(&format!(
            "terminating {} (PID {})",
            holder.holder.name, holder.holder.pid
        )));
    }
    Ok(())
}

fn collect_install_binaries(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut listing = Vec::new();
    for dir in INSTALL_BINARY_DIRS.map(|dir| root.join(dir)) {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "reading install directory {}: {error}",
                    dir.display()
                ))
            }
        };
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                listing.push(entry.path());
            }
        }
    }
    Ok(install_binaries(root, listing))
}

fn wide_name(value: &[u16]) -> String {
    String::from_utf16_lossy(
        &value[..value
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(value.len())],
    )
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

fn last_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests;
