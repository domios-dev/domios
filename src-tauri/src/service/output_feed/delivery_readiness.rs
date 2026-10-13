use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeliveryReadiness {
    pub(crate) pty_id: String,
    pub(crate) generation: Option<u64>,
    pub(crate) modal_visible: bool,
}

#[derive(Default)]
pub(crate) struct DeliveryReadinessEvents {
    observed: Mutex<HashMap<String, DeliveryReadiness>>,
}

impl DeliveryReadinessEvents {
    pub(crate) fn prime(&self, tab_id: &str, readiness: DeliveryReadiness) {
        self.observed
            .lock()
            .entry(tab_id.into())
            .or_insert(readiness);
    }

    pub(crate) fn observe(&self, tab_id: &str, readiness: DeliveryReadiness) -> bool {
        self.observed
            .lock()
            .insert(tab_id.into(), readiness.clone())
            .is_some_and(|previous| previous != readiness)
    }
}

#[cfg(test)]
mod tests;
