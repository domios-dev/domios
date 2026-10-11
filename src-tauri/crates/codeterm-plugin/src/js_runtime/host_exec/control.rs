use std::cell::RefCell;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Process {
    state: parking_lot::Mutex<(bool, Option<u32>)>,
}

impl Process {
    pub(super) fn cancel(&self) {
        let mut state = self.state.lock();
        state.0 = true;
        if let Some(pid) = state.1 {
            kill(pid);
        }
    }

    fn register(&self, pid: u32) {
        let mut state = self.state.lock();
        state.1 = Some(pid);
        if state.0 {
            kill(pid);
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Process>>> = const { RefCell::new(None) };
}

pub(super) struct Scope(Option<Arc<Process>>);

impl Scope {
    pub(super) fn new(process: Arc<Process>) -> Self {
        Self(CURRENT.with(|current| current.replace(Some(process))))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.0.take()));
    }
}

pub(super) struct ChildRegistration(Option<Arc<Process>>);

pub(super) fn spawned(pid: u32) -> ChildRegistration {
    let process = CURRENT.with(|current| current.borrow().clone());
    if let Some(process) = &process {
        process.register(pid);
    }
    ChildRegistration(process)
}

impl Drop for ChildRegistration {
    fn drop(&mut self) {
        if let Some(process) = &self.0 {
            process.state.lock().1 = None;
        }
    }
}

#[cfg(unix)]
fn kill(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill(pid: u32) {
    let mut command = std::process::Command::new("taskkill");
    codeterm_util::process_util::no_window_std(&mut command);
    let _ = command
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
