use codeterm_service_core::service::ServiceState;

pub fn persist_session(svc: &ServiceState) -> Result<(), String> {
    let result =
        codeterm_service_core::service::session_persist::write_session_snapshot(svc, &svc.mux);
    if matches!(
        result,
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved)
    ) {
        let snapshot = svc.mux.read().to_session_snapshot();
        log::info!(
            "[updater] session snapshot saved ({} workspaces)",
            snapshot.workspaces.len()
        );
    }
    shutdown_save_disposition(result)
}

/// A refused reducing save at shutdown is the guard SUCCEEDING: the fuller
/// desk on disk stays authoritative and the next boot restores it, so the
/// update proceeds. Only a genuine persist failure — where the state of the
/// disk is unknown — may abort the teardown.
fn shutdown_save_disposition(
    result: Result<codeterm_service_core::session::store::SaveOutcome, String>,
) -> Result<(), String> {
    match result {
        Ok(codeterm_service_core::session::store::SaveOutcome::Saved) => Ok(()),
        Ok(codeterm_service_core::session::store::SaveOutcome::RefusedReduction(refusal)) => {
            log::error!(
                "[updater] shutdown snapshot {refusal}; proceeding — next boot restores the fuller persisted desk"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reducing_snapshot_at_shutdown_does_not_abort_update() {
        use codeterm_service_core::session::store::{ReductionRefusal, SaveOutcome};

        assert!(
            shutdown_save_disposition(Ok(SaveOutcome::RefusedReduction(ReductionRefusal {
                dropped_workspaces: 2,
                dropped_tabs: 0,
                quarantine: "unused in Storage".into(),
            })))
            .is_ok(),
            "a refused reduction must not abort the shutdown/update path"
        );
    }

    #[test]
    fn captured_desk_disposition_continues_on_refusal_and_aborts_on_error() {
        use codeterm_service_core::session::store::{ReductionRefusal, SaveOutcome};

        let refusal = SaveOutcome::RefusedReduction(ReductionRefusal {
            dropped_workspaces: 1,
            dropped_tabs: 1,
            quarantine: "unused in Storage".into(),
        });
        assert!(shutdown_save_disposition(Ok(refusal)).is_ok());
        assert!(shutdown_save_disposition(Err("commit outcome unknown".into())).is_err());
    }

    #[test]
    fn genuine_persist_failure_at_shutdown_still_aborts() {
        assert!(
            shutdown_save_disposition(Err("storage commit failed".into())).is_err(),
            "an unknown-disk-state failure keeps the cancel semantics"
        );
    }

    #[test]
    fn mem_checkpoint_returns_pragma_row() {
        let mem = codeterm_service_core::mem::MemManager::open_in_memory().unwrap();
        let (busy, log_frames, checkpointed) = mem
            .checkpoint_and_close()
            .expect("checkpoint should succeed on a fresh in-memory db");
        // mode"). We only assert the call path is wired.
        assert!(busy >= -1);
        assert!(log_frames >= -1);
        assert!(checkpointed >= -1);
    }
}
