use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

#[derive(Default)]
pub struct CuratorReconciliations(Mutex<BTreeMap<String, Weak<Mutex<()>>>>);

impl CuratorReconciliations {
    pub fn for_plan(&self, id: &str) -> Arc<Mutex<()>> {
        let mut plans = self.0.lock();
        plans.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = plans.get(id).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Mutex::new(()));
        plans.insert(id.to_owned(), Arc::downgrade(&gate));
        gate
    }
}
