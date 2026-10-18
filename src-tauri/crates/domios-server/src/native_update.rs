use std::sync::Arc;

use codeterm_commands::update::single_flight::{global, UpdateLease};
use codeterm_service_core::{
    update_state::UpdatePhase, UpdateStateHandle,
};
use codeterm_types::{
    native_update::{
        NativeUpdateAction, NativeUpdateExecutor, NativeUpdateRefusal, NativeUpdateRequest,
        NativeUpdateResult, NativeUpdateState,
    },
    server_bootstrap::{ServerComponentManifest, ServerReady},
};

struct FailClosedUpdateLease(Option<UpdateLease<'static>>);

impl FailClosedUpdateLease {
    fn release(mut self) {
        self.0.take();
    }
}

impl Drop for FailClosedUpdateLease {
    fn drop(&mut self) {
        if let Some(lease) = self.0.take() {
            std::mem::forget(lease);
        }
    }
}

pub fn start_native_update(
    request: NativeUpdateRequest,
    ready: &ServerReady,
    installed: &ServerComponentManifest,
    state: UpdateStateHandle,
    executor: Arc<dyn NativeUpdateExecutor>,
) -> Result<tokio::task::JoinHandle<NativeUpdateResult>, NativeUpdateRefusal> {
    request.validate()?;
    if !request.matches_current_server(ready, installed) {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    ensure_supported_action(request.action)?;
    let lease = global()
        .try_acquire()
        .ok_or(NativeUpdateRefusal::AlreadyInFlight)?;
    {
        let mut state = state.lock();
        state.clear_error();
        state.set_phase(UpdatePhase::Applying);
    }
    Ok(tokio::spawn(run_client_update(
        request,
        state,
        executor,
        FailClosedUpdateLease(Some(lease)),
    )))
}

async fn run_client_update(
    request: NativeUpdateRequest,
    state: UpdateStateHandle,
    executor: Arc<dyn NativeUpdateExecutor>,
    lease: FailClosedUpdateLease,
) -> NativeUpdateResult {
    let mut result = executor.dispatch(request.clone()).await;
    let mut previous = None;
    let mut after_sequence = 0;
    loop {
        if result
            .validate_after(&request, previous.as_ref())
            .is_err()
        {
            result = executor
                .observe(request.operation_id, after_sequence)
                .await;
            continue;
        }
        after_sequence = result.sequence;
        previous = Some(result.clone());
        match result.state.clone() {
            NativeUpdateState::Completed { install } => {
                result.state = NativeUpdateState::InstalledAwaitingVerification { install };
            }
            NativeUpdateState::Verified { .. } => {
                state.lock().set_phase(UpdatePhase::Success);
                lease.release();
                return result;
            }
            NativeUpdateState::Rejected { reason } => {
                state
                    .lock()
                    .set_error(format!("Native update refused: {reason:?}"));
                lease.release();
                return result;
            }
            NativeUpdateState::Admitted
            | NativeUpdateState::InProgress { .. }
            | NativeUpdateState::Uncertain { .. }
            | NativeUpdateState::InstalledAwaitingVerification { .. } => {}
        }
        result = executor
            .observe(request.operation_id, after_sequence)
            .await;
    }
}

fn ensure_supported_action(action: NativeUpdateAction) -> Result<(), NativeUpdateRefusal> {
    if action.requires_server_handoff() {
        Err(NativeUpdateRefusal::DurableHandoffUnavailable)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_handoff_refuses_without_a_durable_continuation_owner() {
        assert_eq!(
            ensure_supported_action(NativeUpdateAction::Server),
            Err(NativeUpdateRefusal::DurableHandoffUnavailable)
        );
        assert_eq!(
            ensure_supported_action(NativeUpdateAction::Pair),
            Err(NativeUpdateRefusal::DurableHandoffUnavailable)
        );
        assert_eq!(ensure_supported_action(NativeUpdateAction::ClientOnly), Ok(()));
    }
}
