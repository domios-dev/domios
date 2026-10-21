use std::{
    io::{self, BufRead, Read},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use codeterm_types::server_bootstrap::last_diagnostic_line;

/// How the job object holding the server, if any, ties its lifetime to the launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobMembership {
    Outside,
    SurvivesLauncher,
    KilledWithLauncher,
}

#[cfg(any(windows, test))]
pub(crate) fn job_membership(in_job: bool, kill_on_job_close: bool) -> JobMembership {
    match (in_job, kill_on_job_close) {
        (false, _) => JobMembership::Outside,
        (true, false) => JobMembership::SurvivesLauncher,
        (true, true) => JobMembership::KilledWithLauncher,
    }
}

/// Desktop shells run every process inside a job; only one that kills on close ends the server with its launcher.
pub(crate) fn job_refusal(membership: JobMembership) -> Option<&'static str> {
    (membership == JobMembership::KilledWithLauncher).then_some(
        "Server belongs to a job that kills it when its launcher closes (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)",
    )
}

pub fn ensure_detached() -> io::Result<JobMembership> {
    #[cfg(unix)]
    if unsafe { libc::getsid(0) != libc::getpid() } {
        return Err(io::Error::other("Server does not own its detached session"));
    }
    let membership = current_job_membership()?;
    match job_refusal(membership) {
        Some(reason) => Err(io::Error::other(reason)),
        None => Ok(membership),
    }
}

#[cfg(not(windows))]
fn current_job_membership() -> io::Result<JobMembership> {
    Ok(JobMembership::Outside)
}

#[cfg(windows)]
fn current_job_membership() -> io::Result<JobMembership> {
    use windows_sys::Win32::System::{
        JobObjects::{
            IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::GetCurrentProcess,
    };
    let mut in_job = 0;
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if in_job == 0 {
        return Ok(job_membership(false, false));
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("Server job limits are unreadable: {error}"),
        ));
    }
    let flags = limits.BasicLimitInformation.LimitFlags;
    Ok(job_membership(
        true,
        flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
    ))
}

/// A job without breakaway permission refuses CREATE_BREAKAWAY_FROM_JOB outright.
#[cfg(any(windows, test))]
pub(crate) fn retry_without_breakaway(error: &io::Error) -> bool {
    const ERROR_ACCESS_DENIED: i32 = 5;
    error.raw_os_error() == Some(ERROR_ACCESS_DENIED)
}

/// The launcher stops reading once the server is ready; later writes must not hit its closed pipe.
pub fn release_launch_stderr() -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let null = std::fs::OpenOptions::new().write(true).open("/dev/null")?;
        if unsafe { libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::IntoRawHandle;
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE},
        };
        let null = std::fs::OpenOptions::new()
            .write(true)
            .open(r"\\.\NUL")?
            .into_raw_handle();
        let previous = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if unsafe { SetStdHandle(STD_ERROR_HANDLE, null) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // Closing it could recycle the value under a concurrent stderr writer; it only stops being inherited.
        if !previous.is_null() && previous != INVALID_HANDLE_VALUE {
            super::exclude_bootstrap_handle_from_children(previous)?;
        }
    }
    Ok(())
}

const STDERR_LINE_BYTES: u64 = 4 * 1024;

pub(crate) fn track_last_line(reader: impl Read, last: &Mutex<Option<String>>) {
    let mut reader = io::BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut reader)
            .take(STDERR_LINE_BYTES)
            .read_until(b'\n', &mut line)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                if let Some(text) = last_diagnostic_line(&line) {
                    *last.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text);
                }
            }
        }
    }
}

/// Drains a child's stderr for its whole life so a chatty child never blocks on a full pipe.
pub(crate) struct StderrTail {
    last: Arc<Mutex<Option<String>>>,
    closed: mpsc::Receiver<()>,
}

impl StderrTail {
    pub(crate) fn follow(stderr: Option<impl Read + Send + 'static>) -> Self {
        let last = Arc::new(Mutex::new(None));
        let (sender, closed) = mpsc::channel();
        if let Some(stderr) = stderr {
            let tracked = last.clone();
            std::thread::spawn(move || {
                track_last_line(stderr, &tracked);
                let _ = sender.send(());
            });
        }
        Self { last, closed }
    }

    /// A grandchild may still hold the pipe after the child died, so its close is awaited only briefly.
    pub(crate) fn last_line(self, close_deadline: Duration) -> Option<String> {
        let _ = self.closed.recv_timeout(close_deadline);
        self.last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

pub(crate) fn with_stderr_detail(error: io::Error, detail: Option<String>) -> io::Error {
    match detail {
        Some(detail) => io::Error::new(error.kind(), format!("{error}: {detail}")),
        None => error,
    }
}

#[cfg(test)]
mod tests;
