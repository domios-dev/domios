use std::sync::{Arc, Weak};

use codeterm_service_core::ports::orchestration::{OrchestrationPort, TeamCloseFuture};
use codeterm_service_core::service::{
    ExtensionsBuilder, ServiceContext, ServiceState, TeamMutationCaller,
};

use crate::team::TeamExt;
use crate::{CartographerAttachLocks, OrchestratorAttachLocks};

/// Weak, because the port lives in the context's own extensions.
pub(crate) struct OrchestrationPortImpl(Weak<ServiceContext>);

impl OrchestrationPortImpl {
    fn svc(&self) -> ServiceState {
        self.0
            .upgrade()
            .expect("orchestration port outlived its ServiceContext")
    }
}

pub fn register_orchestration() {
    codeterm_service_core::ports::orchestration::register_orchestration(
        |svc| {
            // Fail fast at install, as required() did: both lock keys are registered extensions.
            svc.ext::<CartographerAttachLocks>();
            svc.ext::<OrchestratorAttachLocks>();
            Arc::new(OrchestrationPortImpl(Arc::downgrade(&svc)))
        },
        register_extensions,
    );
}

fn register_extensions(extensions: &mut ExtensionsBuilder) {
    extensions.insert(CartographerAttachLocks(Default::default()));
    extensions.insert(OrchestratorAttachLocks(Default::default()));
}

impl OrchestrationPort for OrchestrationPortImpl {
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture {
        let svc = self.svc();
        Box::pin(async move {
            let result = svc.close_team(&team_id, caller, force).await?;
            let preserved: Vec<_> = result
                .inventory
                .kept
                .iter()
                .filter_map(|lane| lane.path.as_deref().or(lane.branch.as_deref()))
                .collect();
            if !preserved.is_empty() {
                svc.notify_event(
                    "close-lanes-preserved",
                    serde_json::json!({
                        "paths": preserved,
                        "workspace_id": result.team.workspace_id,
                    }),
                );
            }
            Ok(())
        })
    }
}
