use std::sync::{Arc, OnceLock};

use axum::Router;

/// Startup state the REST router is built from; one field per `AppState` field.
pub(crate) struct RestInputs {
    pub svc: crate::service::ServiceState,
    pub cron: crate::CronState,
    pub cron_wake: crate::CronWake,
    pub api_port: u16,
    pub update_state: crate::UpdateStateHandle,
    pub config: crate::config::ConfigState,
    pub mesh: crate::MeshStateHandle,
    pub keepawake: crate::KeepAwakeStateHandle,
    pub resize_owner: crate::ResizeOwnerState,
    pub rest_api: crate::startup::RestApiControlState,
    pub test_clock: Option<Arc<crate::cron::clock::MockClock>>,
    pub events: codeterm_events::EventBroadcasterState,
    pub web_dist: Option<std::path::PathBuf>,
    pub snapshot_bound: crate::startup::snapshot_bound::SnapshotBound,
    pub bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
}

/// REST entry points the root registers before `run`.
pub struct RestHooks {
    pub build_router: fn(RestInputs) -> Router,
}

static HOOKS: OnceLock<RestHooks> = OnceLock::new();

pub fn register(hooks: RestHooks) {
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
