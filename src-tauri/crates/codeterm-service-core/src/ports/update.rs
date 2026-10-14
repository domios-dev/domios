use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::app_runtime::AppHandle;
use crate::update_state::AvailabilitySnapshot;
use crate::UpdateStateHandle;

pub(crate) type CheckFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AvailabilitySnapshot, String>> + Send + 'a>>;

/// Updater entry points startup calls; the root registers them before `run`.
pub(crate) struct UpdateHooks {
    pub(crate) bind_status_emitter: fn(AppHandle),
    pub(crate) check: for<'a> fn(
        &'a AppHandle,
        &'a UpdateStateHandle,
        Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
        bool,
    ) -> CheckFuture<'a>,
}

static HOOKS: OnceLock<UpdateHooks> = OnceLock::new();

pub fn register(hooks: UpdateHooks) {
    let _ = HOOKS.set(hooks);
}

pub(crate) fn hooks() -> &'static UpdateHooks {
    HOOKS
        .get()
        .expect("update hooks are registered before startup runs")
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_hooks_are_send_and_sync() {
        super::super::assert_port::<super::UpdateHooks>();
    }
}
