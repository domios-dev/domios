use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

pub use codeterm_service_core::service::curator_reconciliations::CuratorReconciliations;

#[cfg(test)]
pub(super) mod test_boundary {
    use super::*;
    use codeterm_service_core::service::ServiceContext;

    pub struct Probe {
        pub plan_id: String,
        pub snapshot: Arc<dyn Fn() + Send + Sync>,
        pub contended: Arc<dyn Fn() + Send + Sync>,
    }

    #[derive(Default)]
    pub struct Boundary(pub Mutex<Option<Arc<Probe>>>);

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
