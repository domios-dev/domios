use crate::{
    store,
    workers::{CreatedEdge, GraphWorkerRegistry, NodeAction},
    MemManager,
};
use std::sync::Arc;

impl MemManager {
    pub fn attach_worker_registry(&self, registry: &Arc<GraphWorkerRegistry>) {
        *self.run_registry.write() = Arc::downgrade(registry);
    }

    pub(crate) fn record_run_diary(&self, diary_id: &str) {
        let Some(worker_id) = store::current_write_worker() else {
            return;
        };
        let Some(registry) = self.run_registry.read().upgrade() else {
            return;
        };
        registry.bind_diary(&worker_id, diary_id);
    }

    pub(crate) fn record_run_mutation(
        &self,
        nodes: &[(String, NodeAction)],
        edges: &[CreatedEdge],
    ) {
        let Some(worker_id) = store::current_write_worker() else {
            return;
        };
        let Some(registry) = self.run_registry.read().upgrade() else {
            return;
        };
        registry.record_mutation(&worker_id, nodes, edges);
    }
}

#[cfg(test)]
mod tests;
