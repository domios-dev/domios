use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

const GLOBAL_LIMIT: usize = 16;
const PLUGIN_LIMIT: usize = 4;
static ACTIVE: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Mutex::default);

pub(super) struct Permit(String);

impl Permit {
    pub(super) fn enter(plugin: &str) -> Result<Self, String> {
        let mut active = ACTIVE.lock();
        if active.values().sum::<usize>() >= GLOBAL_LIMIT
            || active.get(plugin).copied().unwrap_or(0) >= PLUGIN_LIMIT
        {
            return Err("host I/O job admission is busy".into());
        }
        *active.entry(plugin.into()).or_default() += 1;
        Ok(Self(plugin.into()))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut active = ACTIVE.lock();
        if let Some(count) = active.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                active.remove(&self.0);
            }
        }
    }
}
