use crate::service::messenger::AgentDeliveryPayload;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

type Submit = Arc<dyn Fn(&AgentDeliveryPayload) -> Result<(), String> + Send + Sync>;
type Finished = Arc<dyn Fn() + Send + Sync>;
static BOUNDARIES: LazyLock<Mutex<HashMap<String, (Submit, Finished)>>> =
    LazyLock::new(Mutex::default);

pub(crate) struct Boundary(String);

impl Boundary {
    pub(crate) fn install(tab: &str, submit: Submit, finished: Finished) -> Self {
        assert!(BOUNDARIES
            .lock()
            .insert(tab.into(), (submit, finished))
            .is_none());
        Self(tab.into())
    }
}

impl Drop for Boundary {
    fn drop(&mut self) {
        BOUNDARIES.lock().remove(&self.0);
    }
}

pub(crate) fn submit(tab: &str, payload: &AgentDeliveryPayload) -> Option<Result<(), String>> {
    let (submit, _) = BOUNDARIES.lock().get(tab).cloned()?;
    Some(submit(payload))
}

pub(super) fn finished(tab: &str) {
    let boundary = BOUNDARIES.lock().get(tab).cloned();
    if let Some((_, finished)) = boundary {
        finished();
    }
}
