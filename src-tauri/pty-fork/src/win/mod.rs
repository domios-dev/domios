use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::jobapi2::TerminateJobObject;
use winapi::um::minwinbase::STILL_ACTIVE;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::{INFINITE, WAIT_OBJECT_0};

pub mod conpty;
mod procthreadattr;
mod psuedocon;

use filedescriptor::OwnedHandle;

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
    job: OwnedHandle,
    tree_termination_requested: bool,
}

impl WinChild {
    pub(super) fn new(proc: OwnedHandle, job: OwnedHandle) -> Self {
        Self {
            proc: Mutex::new(proc),
            job,
            tree_termination_requested: false,
        }
    }

    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let mut status: DWORD = 0;
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            if status == STILL_ACTIVE {
                Ok(None)
            } else if self.tree_termination_requested {
                match unsafe { WaitForSingleObject(self.job.as_raw_handle() as _, 0) } {
                    WAIT_OBJECT_0 => Ok(Some(ExitStatus::with_exit_code(status))),
                    WAIT_TIMEOUT => Ok(None),
                    _ => Err(IoError::last_os_error()),
                }
            } else {
                Ok(Some(ExitStatus::with_exit_code(status)))
            }
        } else {
            Ok(None)
        }
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            self.tree_termination_requested = true;
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
        Box::new(WinChildKiller { proc, job })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: OwnedHandle,
    job: OwnedHandle,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateJobObject(self.job.as_raw_handle() as _, 1) };
        let err = IoError::last_os_error();
        if res != 0 {
            Ok(())
        } else {
            unsafe { TerminateProcess(self.proc.as_raw_handle() as _, 1) };
            Err(err)
        }
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.try_clone().unwrap();
        let job = self.job.try_clone().unwrap();
        Box::new(WinChildKiller { proc, job })
    }
}

impl Child for WinChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Ok(Some(status)) = self.try_wait() {
            return Ok(status);
        }
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        unsafe {
            WaitForSingleObject(proc.as_raw_handle() as _, INFINITE);
            if self.tree_termination_requested {
                let result = WaitForSingleObject(self.job.as_raw_handle() as _, INFINITE);
                if result != WAIT_OBJECT_0 {
                    return Err(IoError::last_os_error());
                }
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

                let owned = if self.tree_termination_requested {
                    self.job.try_clone()?
                } else {
                    self.proc.lock().unwrap().try_clone()?
                };
                let handle = WaiterHandle(owned.as_raw_handle(), owned);

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    drop(handle.1);
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}
