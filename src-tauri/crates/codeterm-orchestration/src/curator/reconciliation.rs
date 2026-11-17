pub use codeterm_service_core::service::curator_reconciliations::CuratorReconciliations;

#[cfg(test)]
pub(super) mod test_boundary {
    use codeterm_service_core::service::ServiceContext;
    use parking_lot::Mutex;
    use std::sync::Arc;

    pub struct Probe {
        pub plan_id: String,
        pub snapshot: Arc<dyn Fn() + Send + Sync>,
        pub contended: Arc<dyn Fn() + Send + Sync>,
    }

    #[derive(Default)]
    pub struct Boundary(pub Mutex<Option<Arc<Probe>>>);

    pub(crate) fn observe(service: &ServiceContext, id: &str, snapshot: bool) {
        let probe = service.ext_or_init(Boundary::default).0.lock().clone();
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
