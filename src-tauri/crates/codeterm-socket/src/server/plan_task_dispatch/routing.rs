use codeterm_service_core::plan::Task;
use codeterm_service_core::service::SpawnIdentityProvenance;

pub(super) fn dispatch_worker_identity(
    task: &Task,
) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = codeterm_service_core::service::planned_task_title(task);
    let provenance = title
        .as_ref()
        .map(|_| SpawnIdentityProvenance::UserSupplied);
    (title, provenance)
}
