use super::{is_owned_descendant, same_windows_path, ProcessRecord};
use std::collections::HashMap;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::*;

fn creation(handle: &OwnedHandle) -> io::Result<u64> {
    let mut born = FILETIME::default();
    let mut exit = born;
    let mut kernel = born;
    let mut user = born;
    if unsafe {
        GetProcessTimes(
            handle.as_raw_handle(),
            &mut born,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(born.dwHighDateTime) << 32) | u64::from(born.dwLowDateTime))
}

fn open(pid: u32, access: u32) -> io::Result<OwnedHandle> {
    let raw = unsafe { OpenProcess(access, 0, pid) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn executable(handle: &OwnedHandle) -> Option<PathBuf> {
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(handle.as_raw_handle(), 0, image.as_mut_ptr(), &mut length)
    } == 0
    {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &image[..length as usize],
    )))
}

pub fn process_table() -> io::Result<HashMap<u32, ProcessRecord>> {
    let captured = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos()
        / 100
        + 116_444_736_000_000_000;
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw.cast() == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    if unsafe { Process32FirstW(snapshot.as_raw_handle().cast(), &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut processes = HashMap::new();
    loop {
        if let Ok(handle) = open(entry.th32ProcessID, PROCESS_QUERY_LIMITED_INFORMATION) {
            if let Ok(created) = creation(&handle) {
                if u128::from(created) <= captured {
                    processes.insert(
                        entry.th32ProcessID,
                        ProcessRecord {
                            created,
                            parent_pid: entry.th32ParentProcessID,
                            executable: executable(&handle),
                        },
                    );
                }
            }
        }
        if unsafe { Process32NextW(snapshot.as_raw_handle().cast(), &mut entry) } == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                Ok(processes)
            } else {
                Err(error)
            };
        }
    }
}

#[derive(Default)]
pub struct OwnedProcessTree(Vec<OwnedHandle>);

impl OwnedProcessTree {
    pub fn capture(root: (u32, u64), images: &[PathBuf]) -> io::Result<Self> {
        let table = process_table()?;
        if !is_owned_descendant(root, root, images, &table) {
            return Err(io::Error::other("Owned server identity is no longer provable"));
        }
        let mut handles = Vec::new();
        for (&pid, process) in &table {
            if pid == root.0 || !is_owned_descendant(root, (pid, process.created), images, &table) {
                continue;
            }
            let handle = match open(
                pid,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            ) {
                Ok(handle) => handle,
                Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
                    continue
                }
                Err(error) => return Err(error),
            };
            if creation(&handle)? != process.created {
                continue;
            }
            if executable(&handle)
                .is_some_and(|image| images.iter().any(|owned| same_windows_path(&image, owned)))
            {
                handles.push(handle);
            }
        }
        Ok(Self(handles))
    }

    pub fn capture_current_server() -> io::Result<Self> {
        let pid = std::process::id();
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let image = std::env::current_exe()?.canonicalize()?;
        let root = image
            .parent()
            .and_then(|parent| parent.parent())
            .ok_or_else(|| io::Error::other("Server install root unavailable"))?;
        let images = [
            image.clone(),
            root.join("conpty/x64/OpenConsole.exe"),
            root.join("conpty/arm64/OpenConsole.exe"),
        ];
        Self::capture((pid, creation(&handle)?), &images)
    }

    pub fn terminate_remaining(self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        for handle in &self.0 {
            if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_OBJECT_0
                && unsafe { TerminateProcess(handle.as_raw_handle(), 1) } == 0
                && unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_OBJECT_0
            {
                return Err(io::Error::last_os_error());
            }
        }
        for handle in self.0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if unsafe {
                WaitForSingleObject(
                    handle.as_raw_handle(),
                    remaining.as_millis().min(u128::from(u32::MAX - 1)) as u32,
                )
            } != WAIT_OBJECT_0
            {
                return Err(io::Error::other("Owned install process tree exit deadline"));
            }
        }
        Ok(())
    }
}
