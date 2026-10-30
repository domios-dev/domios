use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

#[derive(Default)]
pub(crate) struct CuratorReconciliations(Mutex<BTreeMap<String, Weak<Mutex<()>>>>);

impl CuratorReconciliations {
    pub(super) fn for_plan(&self, id: &str) -> Arc<Mutex<()>> {
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

#[cfg(test)]
pub(super) mod test_boundary {
    use super::*;
    use crate::service::ServiceContext;

    pub(crate) struct Probe {
        pub plan_id: String,
        pub snapshot: Arc<dyn Fn() + Send + Sync>,
        pub contended: Arc<dyn Fn() + Send + Sync>,
    }

    #[derive(Default)]
    pub(crate) struct Boundary(pub Mutex<Option<Arc<Probe>>>);

    pub(crate) fn observe(service: &ServiceContext, id: &str, snapshot: bool) {
        let probe = service
            .extensions
            .get_or_init(Boundary::default)
            .0
            .lock()
            .clone();
        if let Some(probe) = probe.filter(|probe| probe.plan_id == id) {
            if snapshot {
                (probe.snapshot)();
            } else {
                (probe.contended)();
            }
        }
    }
}

#[cfg(test)]
mod tests;
