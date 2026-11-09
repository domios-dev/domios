use std::cell::RefCell;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Process {
    state: parking_lot::Mutex<(bool, Option<super::child::Target>)>,
}

impl Process {
    pub(super) fn cancel(&self) {
        let mut state = self.state.lock();
        state.0 = true;
        if let Some(target) = &state.1 {
            target.terminate();
        }
    }

    fn register(&self, target: super::child::Target) {
        let mut state = self.state.lock();
        if state.0 {
            target.terminate();
        }
        state.1 = Some(target);
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

pub(super) fn spawned(target: super::child::Target) -> ChildRegistration {
    let process = CURRENT.with(|current| current.borrow().clone());
    if let Some(process) = &process {
        process.register(target);
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
