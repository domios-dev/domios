use std::ops::{Deref, DerefMut};
use std::process::{Child, Command, ExitStatus};
#[cfg(windows)]
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Clone)]
pub(super) struct Target {
    #[cfg(unix)]
    pid: u32,
    #[cfg(windows)]
    job: Arc<codeterm_util::windows_job::Job>,
}

impl Target {
    pub(super) fn terminate(&self) {
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
    }
}

pub(super) struct Owned {
    child: Child,
    target: Target,
    readers: Vec<JoinHandle<()>>,
    finished: bool,
}

impl Owned {
    pub(super) fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const CREATE_SUSPENDED: u32 = 0x0000_0004;
            command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn()?;
        #[cfg(windows)]
        let target = match codeterm_util::windows_job::Job::attach_std(&child) {
            Ok(job) => Target { job: Arc::new(job) },
            Err(error) => {
                let mut failed = child;
                let _ = failed.kill();
                let _ = failed.wait();
                return Err(std::io::Error::other(error));
            }
        };
        #[cfg(unix)]
        let target = Target { pid: child.id() };
        Ok(Self {
            child,
            target,
            readers: Vec::new(),
            finished: false,
        })
    }

    pub(super) fn target(&self) -> Target {
        self.target.clone()
    }

    pub(super) fn add_reader(&mut self, reader: JoinHandle<()>) {
        self.readers.push(reader);
    }

    pub(super) fn terminate(&self) {
        self.target.terminate();
    }

    pub(super) fn finish(&mut self) -> std::io::Result<ExitStatus> {
        #[cfg(windows)]
        self.target.terminate();
        #[cfg(unix)]
        if self.child.try_wait()?.is_none() {
            self.target.terminate();
        }
        let status = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        self.finished = true;
        status
    }
}

impl Deref for Owned {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl DerefMut for Owned {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

#[cfg(test)]
mod tests;
