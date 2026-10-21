use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};

pub struct ProcessSlot<T> {
    state: Mutex<State<T>>,
}

struct State<T> {
    closed: bool,
    process: Option<T>,
}

impl<T> ProcessSlot<T> {
    pub fn new(process: Option<T>) -> Self {
        Self {
            state: Mutex::new(State {
                closed: false,
                process,
            }),
        }
    }

    pub fn lock(&self) -> MappedMutexGuard<'_, Option<T>> {
        MutexGuard::map(self.state.lock(), |state| &mut state.process)
    }

    pub fn close(&self) -> Option<T> {
        let mut state = self.state.lock();
        state.closed = true;
        state.process.take()
    }

    pub fn install(&self, process: T, stop: impl FnOnce(&T)) -> bool {
        {
            let mut state = self.state.lock();
            if !state.closed {
                state.process = Some(process);
                return true;
            }
        }
        stop(&process);
        false
    }
}

#[cfg(test)]
mod tests;
