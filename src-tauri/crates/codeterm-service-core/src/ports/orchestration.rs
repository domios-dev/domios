use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::service::extensions::{Extensions, ExtensionsBuilder};
use crate::service::{ServiceState, TeamMutationCaller};

pub type TeamCloseFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Calls the core makes up into Team and orchestration logic.
pub trait OrchestrationPort: Send + Sync {
    /// Async by rule-3 exception: it runs the existing close body; the caller awaits it holding no guard.
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture;
}

pub type Factory = fn(ServiceState) -> Arc<dyn OrchestrationPort>;
pub type ExtensionRegistrar = fn(&mut ExtensionsBuilder);

static FACTORY: OnceLock<(Factory, ExtensionRegistrar)> = OnceLock::new();

/// Per-context slot, filled once right after the context is wrapped in its `Arc`.
#[derive(Default)]
pub(crate) struct InstalledOrchestration(OnceLock<Arc<dyn OrchestrationPort>>);

pub fn register_orchestration(make: Factory, extensions: ExtensionRegistrar) {
    let _ = FACTORY.set((make, extensions));
}

pub(crate) fn register_extensions(builder: &mut ExtensionsBuilder) {
    let (_, extensions) = FACTORY
        .get()
        .expect("orchestration port is registered before the service starts");
    extensions(builder);
}

pub fn install(svc: &ServiceState) {
    let (make, _) = FACTORY
        .get()
        .expect("orchestration port is registered before the service starts");
    fill(svc, make(svc.clone()));
}

fn fill(svc: &ServiceState, port: Arc<dyn OrchestrationPort>) {
    let _ = svc.extensions.get::<InstalledOrchestration>().0.set(port);
}

pub(crate) fn installed(extensions: &Extensions) -> &Arc<dyn OrchestrationPort> {
    extensions
        .get::<InstalledOrchestration>()
        .0
        .get()
        .expect("orchestration port is installed after ServiceContext construction")
}

/// Fixtures get the registered crate when a test registered it, else the recording fake.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn register_fixture_extensions(builder: &mut ExtensionsBuilder) {
    if let Some((_, extensions)) = FACTORY.get() {
        extensions(builder);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn install_fixture(svc: &ServiceState) {
    match FACTORY.get() {
        Some((make, _)) => fill(svc, make(svc.clone())),
        None => fill(svc, Arc::new(RecordingOrchestration::default())),
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingOrchestration {
    pub closed_teams: parking_lot::Mutex<Vec<(String, TeamMutationCaller, bool)>>,
}

#[cfg(any(test, feature = "test-support"))]
impl OrchestrationPort for RecordingOrchestration {
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture {
        self.closed_teams.lock().push((team_id, caller, force));
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn orchestration_port_is_object_safe_and_thread_safe() {
        super::super::assert_port::<dyn super::OrchestrationPort>();
    }
}
