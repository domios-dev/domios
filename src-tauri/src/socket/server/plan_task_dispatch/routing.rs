use crate::plan::Task;
use crate::service::SpawnIdentityProvenance;

pub(super) fn dispatch_worker_identity(
    task: &Task,
) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = crate::service::planned_task_title(task);
    let provenance = title
        .as_ref()
        .map(|_| SpawnIdentityProvenance::UserSupplied);
    (title, provenance)
}
