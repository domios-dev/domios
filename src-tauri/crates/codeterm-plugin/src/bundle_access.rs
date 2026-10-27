use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Weak};
use std::time::Duration;

use parking_lot::{Mutex, RwLock, RwLockReadGuard};

static GATES: LazyLock<Mutex<HashMap<String, Weak<BundleAccess>>>> = LazyLock::new(Mutex::default);

thread_local! {
    static INSTALLING: RefCell<HashSet<String>> = RefCell::default();
}

pub struct BundleAccess {
    id: String,
    gate: RwLock<()>,
}

struct Installing(String);

impl Drop for Installing {
    fn drop(&mut self) {
        INSTALLING.with(|ids| ids.borrow_mut().remove(&self.0));
    }
}

impl BundleAccess {
    pub fn for_plugin(id: &str) -> Arc<Self> {
        let mut gates = GATES.lock();
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(id).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Self {
            id: id.into(),
            gate: RwLock::new(()),
        });
        gates.insert(id.into(), Arc::downgrade(&gate));
        gate
    }

    pub(crate) fn read(&self) -> Option<RwLockReadGuard<'_, ()>> {
        self.reader(false)
    }

    pub(crate) fn read_until(
        &self,
        deadline: std::time::Instant,
    ) -> Result<Option<RwLockReadGuard<'_, ()>>, &'static str> {
        if INSTALLING.with(|ids| ids.borrow().contains(&self.id)) {
            Ok(None)
        } else {
            self.gate
                .try_read_until(deadline)
                .map(Some)
                .ok_or("plugin bundle access deadline")
        }
    }

    pub(crate) fn read_recursive(&self) -> Option<RwLockReadGuard<'_, ()>> {
        self.reader(true)
    }

    fn reader(&self, recursive: bool) -> Option<RwLockReadGuard<'_, ()>> {
        if INSTALLING.with(|ids| ids.borrow().contains(&self.id)) {
            None
        } else if recursive {
            Some(self.gate.read_recursive())
        } else {
            Some(self.gate.read())
        }
    }

    pub(crate) fn install<T>(
        &self,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _exclusive = self
            .gate
            .try_write_for(Duration::from_secs(5))
            .ok_or_else(|| format!("Plugin {} still has active file readers", self.id))?;
        INSTALLING.with(|ids| ids.borrow_mut().insert(self.id.clone()));
        let _installing = Installing(self.id.clone());
        operation()
    }
}

pub fn read_plugin_files<T>(id: &str, read: impl FnOnce() -> T) -> T {
    let access = BundleAccess::for_plugin(id);
    let _reader = access.read();
    read()
}

#[cfg(test)]
mod tests;
