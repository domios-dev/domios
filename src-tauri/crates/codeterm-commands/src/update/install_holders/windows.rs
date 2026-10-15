use super::model::{
    classify_holder, foreign_notice, same_process_identity, Holder, HolderDisposition,
};
use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_NO_MORE_FILES, FILETIME, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::RestartManager::*;
use windows_sys::Win32::System::Threading::*;

struct Handle(HANDLE);
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

struct PinnedHolder {
    holder: Holder,
    handle: Option<Handle>,
    disposition: HolderDisposition,
}

pub(super) fn inspect(
    release: bool,
    svc: Option<&codeterm_service_core::service::ServiceState>,
) -> Result<(), String> {
    let executable =
        std::fs::canonicalize(std::env::current_exe().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let root = executable.parent().ok_or("install directory unresolved")?;
    inspect_directory(root, release, svc)
}

fn inspect_directory(
    root: &Path,
    release: bool,
    svc: Option<&codeterm_service_core::service::ServiceState>,
) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
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
    clis.push(codeterm_service_core::paths::data_dir().join("bin/codeterm.exe"));
    let helpers = [
        root.join("codeterm-embed.exe"),
        root.join("conpty/x64/OpenConsole.exe"),
        root.join("conpty/arm64/OpenConsole.exe"),
    ];
    let session = Session::register(&files)?;
    loop {
        let parents = process_parents()?;
        let mut pinned = Vec::new();
        for info in session.holders()? {
            if let Some(holder) = pin_holder(&info, &parents, &clis, &helpers, svc)? {
                pinned.push(holder);
            }
        }
        let mut foreign = Vec::new();
        let mut owned = Vec::new();
        for holder in pinned {
            match holder.disposition {
                HolderDisposition::Foreign => foreign.push(holder.holder),
                HolderDisposition::OwnedHelper => owned.push(holder),
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
            return Ok(());
        }
        for holder in &owned {
            terminate(holder)?;
        }
        for holder in owned {
            let handle = holder
                .handle
                .as_ref()
                .ok_or("owned install holder has no exit handle")?;
            if unsafe { WaitForSingleObject(handle.0, INFINITE) } != WAIT_OBJECT_0 {
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
    }
}

fn pin_holder(
    info: &RM_PROCESS_INFO,
    parents: &HashMap<u32, u32>,
    clis: &[PathBuf],
    helpers: &[PathBuf],
    svc: Option<&codeterm_service_core::service::ServiceState>,
) -> Result<Option<PinnedHolder>, String> {
    let pid = info.Process.dwProcessId;
    let mut holder = Holder {
        pid,
        name: wide_name(&info.strAppName),
        executable: None,
        parent_pid: parents.get(&pid).copied(),
        owned_tab: None,
    };
    if holder.name.is_empty() {
        holder.name = "Unknown process".into();
    }
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
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
    if let Some(svc) = svc {
        let owned_pty = { svc.pty.lock().owning_pty(raw.cast()) }.map_err(|error| {
            format!(
                "checking job ownership for {} (PID {pid}): {error}",
                holder.name
            )
        })?;
        if let Some(pty_id) = owned_pty {
            holder.owned_tab = Some(
                svc.mux
                    .read()
                    .snapshot()
                    .tabs
                    .values()
                    .find(|tab| tab.pty_id.as_deref() == Some(pty_id.as_str()))
                    .map(|tab| format!("Tab {} ({})", tab.title, tab.id))
                    .unwrap_or_else(|| format!("PTY {pty_id}")),
            );
        }
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
    let raw = unsafe { OpenProcess(PROCESS_TERMINATE, 0, holder.holder.pid) };
    if raw.is_null() {
        if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        return Err(last_error(&format!(
            "terminating {} (PID {})",
            holder.holder.name, holder.holder.pid
        )));
    }
    let terminate_handle = Handle(raw);
    if unsafe { TerminateProcess(terminate_handle.0, 1) } == 0
        && unsafe { WaitForSingleObject(process.0, 0) } != WAIT_OBJECT_0
    {
        return Err(last_error(&format!(
            "terminating {} (PID {})",
            holder.holder.name, holder.holder.pid
        )));
    }
    Ok(())
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|error| format!("reading install directory {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            collect_files(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn process_parents() -> Result<HashMap<u32, u32>, String> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(last_error("enumerating install-holder parents"));
    }
    let snapshot = Handle(raw);
    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(last_error("reading install-holder parents"));
    }
    let mut parents = HashMap::new();
    loop {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(parents);
            }
            return Err(last_error("advancing install-holder parents"));
        }
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
