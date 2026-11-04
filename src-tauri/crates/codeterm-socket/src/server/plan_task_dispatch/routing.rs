use codeterm_service_core::service::SpawnIdentityProvenance;

pub(super) fn dispatch_worker_identity(
    personality: Option<&str>,
) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = codeterm_service_core::service::spawn_plan_task::planned_task_title_for_personality(
        personality,
    );
    let provenance = title
        .as_ref()
        .map(|_| SpawnIdentityProvenance::UserSupplied);
    (title, provenance)
}
