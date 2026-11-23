use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

mod probe_child;
pub use probe_child::ProbeChild;

/// A kill-on-close job: closing its last handle (even by process death) kills every member.
pub struct Job(usize);
impl Job {
    /// Contains a child spawned suspended, then resumes it.
    pub fn attach(child: &tokio::process::Child) -> Result<Self, String> {
        let job = Self::contain(child)?;
        resume(child)?;
        Ok(job)
    }

    /// Contains a child spawned suspended; the caller resumes it, so its first descendant is already inside.
    pub fn contain(child: &tokio::process::Child) -> Result<Self, String> {
        let process = child.raw_handle().ok_or("Missing child process handle")? as HANDLE;
        Self::contain_handle(process)
    }

    pub fn attach_std(child: &std::process::Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        let process = child.as_raw_handle() as HANDLE;
        let job = Self::contain_handle(process)?;
        resume_handle(process)?;
        Ok(job)
    }

    fn contain_handle(process: HANDLE) -> Result<Self, String> {
        let job = Self::new()?;
        if unsafe { AssignProcessToJobObject(job.0 as HANDLE, process) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(job)
    }

    fn new() -> Result<Self, String> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let job = Self(handle as usize);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(job)
    }
    pub fn terminate(&self) {
        let _ = self.try_terminate();
    }
    pub fn try_terminate(&self) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.0 as HANDLE, 1) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }

    pub fn terminate_and_wait(&self, timeout: std::time::Duration) -> Result<(), String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use std::time::Instant;

        self.try_terminate()?;
        let deadline = Instant::now() + timeout;
        for pid in self.process_ids()? {
            let raw = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    continue;
                }
                return Err(error.to_string());
            }
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            if unsafe { self.contains_process(handle.as_raw_handle()) }? {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let millis = remaining.as_millis().min(u128::from(u32::MAX - 1)) as u32;
                if unsafe { WaitForSingleObject(handle.as_raw_handle(), millis) } != WAIT_OBJECT_0 {
                    return Err("Owned Job exit was not confirmed".into());
                }
            }
        }
        Ok(())
    }

    pub fn process_ids(&self) -> Result<Vec<u32>, String> {
        #[repr(C)]
        struct ProcessIds {
            assigned: u32,
            count: u32,
            ids: [usize; 256],
        }
        let mut list = ProcessIds {
            assigned: 0,
            count: 0,
            ids: [0; 256],
        };
        if unsafe {
            QueryInformationJobObject(
                self.0 as HANDLE,
                JobObjectBasicProcessIdList,
                (&mut list as *mut ProcessIds).cast(),
                std::mem::size_of_val(&list) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if list.assigned as usize > list.ids.len() || list.count > list.assigned {
            return Err("Owned Job process snapshot exceeded its bound".into());
        }
        if list.count == 0 && list.assigned != 0 {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                unsafe { std::mem::zeroed() };
            if unsafe {
                QueryInformationJobObject(
                    self.0 as HANDLE,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if accounting.ActiveProcesses != 0 {
                return Err("Owned Job membership changed during process snapshot".into());
            }
        }
        list.ids[..list.count as usize]
            .iter()
            .map(|id| u32::try_from(*id).map_err(|_| "Invalid owned Job process id".into()))
            .collect()
    }

    /// # Safety
    /// The process handle must remain valid for this call.
    pub unsafe fn contains_process(&self, process: HANDLE) -> Result<bool, String> {
        let mut contained = 0;
        if unsafe { IsProcessInJob(process, self.0 as HANDLE, &mut contained) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(contained != 0)
    }
}
pub fn resume(child: &tokio::process::Child) -> Result<(), String> {
    let process = child.raw_handle().ok_or("Missing child process handle")? as HANDLE;
    resume_handle(process)
}

fn resume_handle(process: HANDLE) -> Result<(), String> {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: HANDLE) -> i32;
    }
    if unsafe { NtResumeProcess(process) } < 0 {
        return Err("Could not resume the suspended child".into());
    }
    Ok(())
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}

pub fn terminate_orphan(pid: u32, start_token: &str) -> bool {
    let process = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if process.is_null() {
        return false;
    }
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let identity =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0 };
    let observed =
        ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)).to_string();
    let result =
        identity && observed == start_token && unsafe { TerminateProcess(process, 1) != 0 };
    unsafe {
        CloseHandle(process);
    }
    result
}
