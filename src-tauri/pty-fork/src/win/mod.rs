use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use winapi::shared::minwindef::DWORD;
use winapi::um::jobapi2::TerminateJobObject;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::{INFINITE, WAIT_OBJECT_0};

pub mod conpty;
mod job_exit;
mod procthreadattr;
mod psuedocon;

use filedescriptor::OwnedHandle;

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
    job: OwnedHandle,
    tree_termination_requested: Arc<AtomicBool>,
}

impl WinChild {
    pub(super) fn new(proc: OwnedHandle, job: OwnedHandle) -> Self {
        Self {
            proc: Mutex::new(proc),
            job,
            tree_termination_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let proc = self.proc.lock().unwrap().try_clone()?;
        match unsafe { WaitForSingleObject(proc.as_raw_handle() as _, 0) } {
            WAIT_OBJECT_0 => {}
            winapi::shared::winerror::WAIT_TIMEOUT => return Ok(None),
            _ => return Err(IoError::last_os_error()),
        }
        if self.tree_termination_requested.load(Ordering::Acquire)
            && !job_exit::is_empty(&self.job)?
        {
            return Ok(None);
        }
        let mut status: DWORD = 0;
        if unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) } == 0 {
            return Err(IoError::last_os_error());
        }
        Ok(Some(ExitStatus::with_exit_code(status)))
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            self.tree_termination_requested
                .store(true, Ordering::Release);
            Ok(())
        } else {
            let proc = self.proc.lock().unwrap().try_clone().unwrap();
            unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
            Err(err)
        }
    }
}

impl ChildKiller for WinChild {
    fn kill(&mut self) -> IoResult<()> {
        self.do_kill()
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        let job = self.job.try_clone().unwrap();
        Box::new(WinChildKiller {
            proc,
            job,
            tree_termination_requested: self.tree_termination_requested.clone(),
        })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: OwnedHandle,
    job: OwnedHandle,
    tree_termination_requested: Arc<AtomicBool>,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            self.tree_termination_requested
                .store(true, Ordering::Release);
            Ok(())
        } else {
            unsafe { TerminateProcess(self.proc.as_raw_handle() as _, 1) };
            Err(err)
        }
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.try_clone().unwrap();
        let job = self.job.try_clone().unwrap();
        Box::new(WinChildKiller {
            proc,
            job,
            tree_termination_requested: self.tree_termination_requested.clone(),
        })
    }
}

impl Child for WinChild {
    fn contains_process(&self, process: RawHandle) -> IoResult<bool> {
        let mut belongs = 0;
        if unsafe {
            winapi::um::jobapi2::IsProcessInJob(
                process as _,
                self.job.as_raw_handle() as _,
                &mut belongs,
            )
        } == 0
        {
            return Err(IoError::last_os_error());
        }
        Ok(belongs != 0)
    }

    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Ok(Some(status)) = self.try_wait() {
            return Ok(status);
        }
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        unsafe {
            if WaitForSingleObject(proc.as_raw_handle() as _, INFINITE) != WAIT_OBJECT_0 {
                return Err(IoError::last_os_error());
            }
            if self.tree_termination_requested.load(Ordering::Acquire) {
                job_exit::wait(&self.job)?;
            }
        }
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
        let res = unsafe { GetProcessId(self.proc.lock().unwrap().as_raw_handle() as _) };
        if res == 0 {
            None
        } else {
            Some(res)
        }
    }

    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        let proc = self.proc.lock().unwrap();
        Some(proc.as_raw_handle())
    }
}

impl std::future::Future for WinChild {
    type Output = anyhow::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<anyhow::Result<ExitStatus>> {
        match self.is_complete() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Err(err) => Poll::Ready(Err(err).context("Failed to retrieve process exit status")),
            Ok(None) => {
                struct WaiterHandle(RawHandle, OwnedHandle);
                unsafe impl Send for WaiterHandle {}

                let owned = self.proc.lock().unwrap().try_clone()?;
                let job = self.job.try_clone()?;
                let tree_termination_requested = self.tree_termination_requested.clone();
                let handle = WaiterHandle(owned.as_raw_handle(), owned);

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    if tree_termination_requested.load(Ordering::Acquire) {
                        let _ = job_exit::wait(&job);
                    }
                    drop(handle.1);
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests;
