use super::*;
use crate::service::parked_user_delivery::handoff::PayloadEnqueued;

type Submit = Arc<
    dyn Fn(Option<&UserSendRecord>, &str, PayloadEnqueued) -> Result<(), DeliveryError>
        + Send
        + Sync,
>;
static BOUNDARIES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Submit>>> =
    std::sync::OnceLock::new();

pub(super) struct QueuedSubmitBoundary(String);

impl QueuedSubmitBoundary {
    pub(super) fn install(tab_id: &str, submit: Submit) -> Self {
        BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .insert(tab_id.into(), submit);
        Self(tab_id.into())
    }
}

impl Drop for QueuedSubmitBoundary {
    fn drop(&mut self) {
        BOUNDARIES
            .get_or_init(Default::default)
            .lock()
            .remove(&self.0);
    }
}

pub(in crate::service::parked_user_delivery) fn boundary(tab_id: &str) -> Option<Submit> {
    BOUNDARIES
        .get_or_init(Default::default)
        .lock()
        .get(tab_id)
        .cloned()
}
