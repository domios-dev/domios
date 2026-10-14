use std::sync::{Arc, OnceLock};

use axum::Router;

/// Startup state the REST router is built from; one field per `AppState` field.
pub(crate) struct RestInputs {
    pub(crate) svc: crate::service::ServiceState,
    pub(crate) cron: crate::CronState,
    pub(crate) cron_wake: crate::CronWake,
    pub(crate) api_port: u16,
    pub(crate) update_state: crate::UpdateStateHandle,
    pub(crate) config: crate::config::ConfigState,
    pub(crate) mesh: crate::MeshStateHandle,
    pub(crate) keepawake: crate::KeepAwakeStateHandle,
    pub(crate) resize_owner: crate::ResizeOwnerState,
    pub(crate) rest_api: crate::startup::RestApiControlState,
    pub(crate) test_clock: Option<Arc<crate::cron::clock::MockClock>>,
    pub(crate) events: codeterm_events::EventBroadcasterState,
    pub(crate) web_dist: Option<std::path::PathBuf>,
    pub(crate) snapshot_bound: crate::startup::snapshot_bound::SnapshotBound,
    pub(crate) bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
}

/// REST entry points the root registers before `run`.
pub(crate) struct RestHooks {
    pub(crate) build_router: fn(RestInputs) -> Router,
}

static HOOKS: OnceLock<RestHooks> = OnceLock::new();

pub(crate) fn register(hooks: RestHooks) {
    let _ = HOOKS.set(hooks);
}

pub(crate) fn hooks() -> &'static RestHooks {
    HOOKS
        .get()
        .expect("rest hooks are registered before startup builds the router")
}

#[derive(Clone)]
pub struct LocalApiRouter(pub Router);

#[cfg(test)]
mod tests {
    #[test]
    fn rest_hooks_are_send_and_sync() {
        super::super::assert_port::<super::RestHooks>();
    }
}
