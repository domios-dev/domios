use super::Boundary;
use std::time::Instant;
use windows_sys::Win32::{Foundation::*, System::Threading::*};

pub(super) fn ids(boundary: &Boundary) -> Result<Vec<u32>, String> {
    boundary.job.process_ids()
}
pub(super) fn terminate(boundary: &Boundary) -> Result<(), String> {
    boundary.job.try_terminate()
}

pub(super) struct Member {
    handle: usize,
    born: u128,
}
impl Member {
    pub(super) fn open(boundary: &Boundary, pid: u32) -> Result<Option<Self>, String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                return Ok(None);
            }
            return Err(error.to_string());
        }
        let mut member = Self {
            handle: handle as usize,
            born: 0,
        };
        if !boundary.job.contains_process(handle)? {
            return Err("Process left the owned Job before observation".into());
        }
        let mut born = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(handle, &mut born, &mut exited, &mut kernel, &mut user) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        member.born = ((u128::from(born.dwHighDateTime)) << 32) | u128::from(born.dwLowDateTime);
        Ok(Some(member))
    }
    pub(super) fn token(&self) -> u128 {
        self.born
    }
    pub(super) fn validate_identity(&self, boundary: &Boundary) -> Result<(), String> {
        if self.exited()? || boundary.job.contains_process(self.handle as HANDLE)? {
            Ok(())
        } else {
            Err("Retained process is outside the owned Job".into())
        }
    }
    pub(super) fn exited(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.handle as HANDLE, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }
    pub(super) fn terminate(&self, boundary: &Boundary) -> Result<(), String> {
        self.validate_identity(boundary)?;
        if self.exited()? {
            return Ok(());
        }
        if unsafe { TerminateProcess(self.handle as HANDLE, 1) } == 0 && !self.exited()? {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    pub(super) fn wait(&self, deadline: Instant) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = remaining
            .as_millis()
            .saturating_add(1)
            .min(u32::MAX as u128 - 1) as u32;
        if unsafe { WaitForSingleObject(self.handle as HANDLE, millis) } == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err("Owned process exit wait failed or reached its deadline".into())
        }
    }
}
impl Drop for Member {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}
