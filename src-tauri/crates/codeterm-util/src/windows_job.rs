use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

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
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let job = Self(handle as usize);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let process = child.raw_handle().ok_or("Missing child process handle")? as HANDLE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(handle, process) } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(job)
    }
    pub fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0 as HANDLE, 1);
        }
    }
}
pub fn resume(child: &tokio::process::Child) -> Result<(), String> {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtResumeProcess(process: HANDLE) -> i32;
    }
    let process = child.raw_handle().ok_or("Missing child process handle")? as HANDLE;
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
