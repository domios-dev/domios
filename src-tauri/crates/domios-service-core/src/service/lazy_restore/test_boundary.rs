use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

type Materialize = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
static MATERIALIZERS: LazyLock<Mutex<HashMap<String, Materialize>>> = LazyLock::new(Mutex::default);

pub(crate) struct Boundary(String);

impl Boundary {
    pub(crate) fn install(tab: &str, materialize: Materialize) -> Self {
        assert!(MATERIALIZERS
            .lock()
            .insert(tab.into(), materialize)
            .is_none());
        Self(tab.into())
    }
}

impl Drop for Boundary {
    fn drop(&mut self) {
        MATERIALIZERS.lock().remove(&self.0);
    }
}

pub(super) fn materialize(tab: &str) -> Option<Result<(), String>> {
    let materialize = MATERIALIZERS.lock().get(tab).cloned()?;
    Some(materialize())
}
