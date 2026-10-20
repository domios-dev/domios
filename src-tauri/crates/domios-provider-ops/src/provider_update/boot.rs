use parking_lot::Mutex;

type Check = Box<dyn FnOnce() + Send>;

/// Holds the startup update check until the restore wave reports completion; either side may arrive first.
pub(super) struct RestoreGate {
    state: Mutex<(bool, Option<Check>)>,
}

impl RestoreGate {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new((false, None)),
        }
    }

    pub(super) fn defer(&self, check: Check) {
        let ready = {
            let mut state = self.state.lock();
            if state.0 {
                Some(check)
            } else {
                state.1 = Some(check);
                None
            }
        };
        if let Some(check) = ready {
            check();
        }
    }

    pub(super) fn open(&self) {
        let check = {
            let mut state = self.state.lock();
            state.0 = true;
            state.1.take()
        };
        if let Some(check) = check {
            check();
        }
    }
}

pub(super) static RESTORE_GATE: RestoreGate = RestoreGate::new();
