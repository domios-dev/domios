use std::process::{Child, Command};
#[cfg(windows)]
use std::time::Instant;

pub(crate) trait ProcessChild {
    fn id(&self) -> u32;
    #[cfg(windows)]
    fn kill(&mut self) -> std::io::Result<()>;
    #[cfg(windows)]
    fn process_handle(&self) -> windows_sys::Win32::Foundation::HANDLE;
}

impl ProcessChild for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }
    #[cfg(windows)]
    fn kill(&mut self) -> std::io::Result<()> {
        Child::kill(self)
    }
    #[cfg(windows)]
    fn process_handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        use std::os::windows::io::AsRawHandle;
        self.as_raw_handle().cast()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundaryCompletion {
    #[cfg(unix)]
    VisibleGroupAbsent,
    #[cfg(windows)]
    OwnedJobEmpty,
    PresentOrUnknown,
}

impl BoundaryCompletion {
    pub(crate) fn confirmed(self) -> bool {
        self != Self::PresentOrUnknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeaderOwnership {
    Unspawned,
    Retained(u32),
    Retired(u32),
    Lost,
}

pub(crate) struct ProcessTree {
    leader: LeaderOwnership,
    #[cfg(test)]
    termination_error: bool,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    port: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(windows)]
    assigned: bool,
}

impl ProcessTree {
    pub(crate) fn prepare(command: &mut Command) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            Ok(Self {
                leader: LeaderOwnership::Unspawned,
                #[cfg(test)]
                termination_error: false,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::{
                Foundation::INVALID_HANDLE_VALUE,
                System::{JobObjects::*, IO::CreateIoCompletionPort},
            };
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut tree = Self {
                leader: LeaderOwnership::Unspawned,
                #[cfg(test)]
                termination_error: false,
                job,
                port: std::ptr::null_mut(),
                assigned: false,
            };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            tree.port =
                unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
            if tree.port.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
                CompletionKey: job.cast(),
                CompletionPort: tree.port,
            };
            if unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectAssociateCompletionPortInformation,
                    (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                    std::mem::size_of_val(&association) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            command.creation_flags(0x08000000 | 0x00000004);
            Ok(tree)
        }
    }

    pub(crate) fn record_spawn(&mut self, child: &impl ProcessChild) -> Result<(), String> {
        if self.leader != LeaderOwnership::Unspawned
            || child.id() <= 1
            || child.id() > i32::MAX as u32
        {
            return Err("invalid updater leader ownership".into());
        }
        self.leader = LeaderOwnership::Retained(child.id());
        Ok(())
    }

    pub(crate) fn attach(&mut self, child: &impl ProcessChild) -> Result<(), String> {
        if self.leader != LeaderOwnership::Retained(child.id()) {
            return Err("updater leader ownership lost before attach".into());
        }
        #[cfg(windows)]
        {
            #[link(name = "ntdll")]
            unsafe extern "system" {
                fn NtResumeProcess(process: windows_sys::Win32::Foundation::HANDLE) -> i32;
            }
            return self.attach_with_resume(child, |process| unsafe { NtResumeProcess(process) });
        }
        #[cfg(not(windows))]
        Ok(())
    }

    #[cfg(windows)]
    fn attach_with_resume(
        &mut self,
        child: &impl ProcessChild,
        resume: impl FnOnce(windows_sys::Win32::Foundation::HANDLE) -> i32,
    ) -> Result<(), String> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        let process = child.process_handle();
        if unsafe { AssignProcessToJobObject(self.job, process) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        self.assigned = true;
        let status = resume(process);
        if status < 0 {
            return Err(format!(
                "could not resume the bounded provider updater: NTSTATUS {status:#x}"
            ));
        }
        Ok(())
    }

    pub(crate) fn reaped(&mut self, child: &impl ProcessChild) {
        self.leader = match self.leader {
            LeaderOwnership::Retained(id) if id == child.id() => LeaderOwnership::Retired(id),
            _ => LeaderOwnership::Lost,
        };
    }

    pub(crate) fn wait_failed(&mut self) {
        self.leader = LeaderOwnership::Lost;
    }

    pub(crate) fn observe_event(&self) -> Result<BoundaryCompletion, String> {
        #[cfg(unix)]
        {
            let id = match self.leader {
                LeaderOwnership::Retained(id) | LeaderOwnership::Retired(id)
                    if id > 1 && id <= i32::MAX as u32 =>
                {
                    id
                }
                _ => return Ok(BoundaryCompletion::PresentOrUnknown),
            };
            let result = unsafe { libc::kill(-(id as i32), 0) };
            let error = std::io::Error::last_os_error();
            // ESRCH covers the original visible group, not escaped or hidden descendants.
            let completion = classify_group_observation(result, error.raw_os_error());
            if result == -1 && !completion.confirmed() {
                return Err(format!(
                    "visible-group completion query unconfirmed: {error}"
                ));
            }
            Ok(completion)
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::*;
            if !self.assigned {
                return Ok(BoundaryCompletion::PresentOrUnknown);
            }
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            if unsafe {
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(if accounting.ActiveProcesses == 0 {
                BoundaryCompletion::OwnedJobEmpty
            } else {
                BoundaryCompletion::PresentOrUnknown
            })
        }
    }

    #[cfg(windows)]
    pub(crate) fn wait_completion(&self, deadline: Instant) -> Result<BoundaryCompletion, String> {
        use windows_sys::Win32::System::IO::GetQueuedCompletionStatus;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("owned Job completion exceeded its failure deadline".into());
            }
            let mut message = 0;
            let mut key = 0;
            let mut overlapped = std::ptr::null_mut();
            let millis = remaining
                .as_millis()
                .saturating_add(1)
                .min(u32::MAX as u128 - 1) as u32;
            if unsafe {
                GetQueuedCompletionStatus(
                    self.port,
                    &mut message,
                    &mut key,
                    &mut overlapped,
                    millis,
                )
            } == 0
            {
                return Err(format!(
                    "owned Job completion wait failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if Instant::now() >= deadline {
                return Err("owned Job event arrived after its failure deadline".into());
            }
            if key != self.job as usize {
                return Err("unexpected owned Job completion key".into());
            }
            // Job notifications are hints; accounting at each event supplies the completion fact.
            if self.observe_event()?.confirmed() {
                return Ok(BoundaryCompletion::OwnedJobEmpty);
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn close_handles(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        for handle in [&mut self.job, &mut self.port] {
            let owned = std::mem::replace(handle, std::ptr::null_mut());
            if !owned.is_null()
                && unsafe { windows_sys::Win32::Foundation::CloseHandle(owned) } == 0
            {
                errors.push(std::io::Error::last_os_error().to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_termination_error(&mut self, enabled: bool) {
        self.termination_error = enabled;
    }

    pub(crate) fn terminate(&self, child: &mut impl ProcessChild) -> Result<(), String> {
        #[cfg(test)]
        if self.termination_error {
            return Err("injected termination API error".into());
        }
        #[cfg(unix)]
        {
            terminate_owned_group(self.leader, child.id(), |pgid| {
                if unsafe { libc::kill(-pgid, libc::SIGKILL) } != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
        }
        #[cfg(windows)]
        {
            if self.assigned {
                if unsafe {
                    windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1)
                } == 0
                {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                Ok(())
            } else if self.leader == LeaderOwnership::Retained(child.id()) {
                child.kill().map_err(|error| error.to_string())
            } else {
                Err("unassigned updater ownership lost before termination".into())
            }
        }
    }
}

#[cfg(unix)]
fn terminate_owned_group(
    ownership: LeaderOwnership,
    child_id: u32,
    signal: impl FnOnce(i32) -> std::io::Result<()>,
) -> Result<(), String> {
    let pgid = termination_target(ownership, child_id)?;
    signal(pgid).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn termination_target(ownership: LeaderOwnership, child_id: u32) -> Result<i32, String> {
    match ownership {
        LeaderOwnership::Retained(id) if id == child_id && id > 1 && id <= i32::MAX as u32 => {
            Ok(id as i32)
        }
        _ => Err("updater leader is reaped or unowned; destructive group cleanup withheld".into()),
    }
}

#[cfg(unix)]
fn classify_group_observation(result: i32, errno: Option<i32>) -> BoundaryCompletion {
    if result == -1 && errno == Some(libc::ESRCH) {
        BoundaryCompletion::VisibleGroupAbsent
    } else {
        BoundaryCompletion::PresentOrUnknown
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Err(error) = self.close_handles() {
            log::error!("[provider-update] owned process handle cleanup failed: {error}");
        }
    }
}

pub(crate) fn owner_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let exists = unsafe { libc::kill(pid as i32, 0) == 0 };
        exists || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return std::io::Error::last_os_error().raw_os_error() != Some(87);
        }
        let mut code = 0;
        let read = unsafe { GetExitCodeProcess(handle, &mut code) };
        unsafe {
            CloseHandle(handle);
        }
        read == 0 || code == 259
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
