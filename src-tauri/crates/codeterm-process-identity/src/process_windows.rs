use codeterm_types::server_bootstrap::ProcessProvenance;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::{FILETIME, WAIT_TIMEOUT},
    System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

pub struct PinnedProcess {
    handle: OwnedHandle,
    pid: u32,
}

impl PinnedProcess {
    pub fn from_child_witness(pid: u32, handle: OwnedHandle) -> io::Result<Self> {
        let result = Self { handle, pid };
        result.ensure_alive()?;
        Ok(result)
    }

    pub fn open(pid: u32) -> io::Result<Self> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = Self {
            handle: unsafe { OwnedHandle::from_raw_handle(handle) },
            pid,
        };
        result.ensure_alive()?;
        Ok(result)
    }

    pub fn wait_exit(&self, deadline: std::time::Duration) -> io::Result<()> {
        let result = unsafe {
            WaitForSingleObject(
                self.handle.as_raw_handle(),
                deadline.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        };
        if result == windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(io::Error::other("Exact process exit deadline"))
        }
    }

    pub fn ensure_alive(&self) -> io::Result<()> {
        if unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } == WAIT_TIMEOUT {
            Ok(())
        } else {
            Err(io::Error::other("Bootstrap peer exited"))
        }
    }

    pub fn wait_for_exit(&self) -> io::Result<()> {
        let result = unsafe { WaitForSingleObject(self.handle.as_raw_handle(), u32::MAX) };
        if result == windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn provenance(&self) -> io::Result<ProcessProvenance> {
        let mut born: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited = born;
        let mut kernel = born;
        let mut user = born;
        if unsafe {
            GetProcessTimes(
                self.handle.as_raw_handle(),
                &mut born,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut image = vec![0u16; 32768];
        let mut count = image.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(
                self.handle.as_raw_handle(),
                0,
                image.as_mut_ptr(),
                &mut count,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let image = String::from_utf16(&image[..count as usize]).map_err(io::Error::other)?;
        self.ensure_alive()?;
        Ok(ProcessProvenance {
            pid: self.pid,
            created: (((born.dwHighDateTime as u64) << 32) | born.dwLowDateTime as u64).to_string(),
            executable: PathBuf::from(image).canonicalize()?,
        })
    }
}
