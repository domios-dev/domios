use super::{RestApiCommand, RestApiControl, RestApiPhase};
use std::time::Duration;
use tokio::sync::oneshot;

pub(crate) struct RestartAcceptance {
    pub(crate) generation: u64,
    pub(crate) response_complete: oneshot::Sender<()>,
}

pub(super) fn accept(
    control: &RestApiControl,
    accepted: oneshot::Sender<Result<u64, String>>,
    response_complete: oneshot::Receiver<()>,
) {
    let reservation = {
        let mut lifecycle = control.lifecycle.lock();
        reserve(
            lifecycle.status.generation,
            &lifecycle.status.phase,
            lifecycle.pending_restart,
        )
        .inspect(|generation| lifecycle.pending_restart = Some(*generation))
    };
    let Ok(generation) = reservation else {
        let _ = accepted.send(reservation);
        return;
    };
    if accepted.send(Ok(generation)).is_err() {
        release(control, generation);
        return;
    }
    let control = control.clone();
    tokio::spawn(async move {
        match tokio::time::timeout(Duration::from_secs(5), response_complete).await {
            Ok(Ok(())) => {
                let (completed, result) = oneshot::channel();
                if control
                    .signal(RestApiCommand::ReservedRestart {
                        generation,
                        completed,
                    })
                    .is_ok()
                {
                    match result.await {
                        Ok(status) => log::debug!("[api] reserved restart completed: {status:?}"),
                        Err(_) => log::error!("[api] reserved restart controller stopped"),
                    }
                }
            }
            _ => log::debug!("[api] restart cancelled before response completion"),
        }
        release(&control, generation);
    });
}

fn reserve(generation: u64, phase: &RestApiPhase, pending: Option<u64>) -> Result<u64, String> {
    if pending.is_some() {
        return Err("Listener restart is already reserved".into());
    }
    if *phase != RestApiPhase::Listening {
        return Err("Listener is not health-proven".into());
    }
    generation
        .checked_add(1)
        .ok_or_else(|| "Listener generation exhausted".into())
}

fn release(control: &RestApiControl, generation: u64) {
    let mut lifecycle = control.lifecycle.lock();
    if lifecycle.pending_restart == Some(generation) {
        lifecycle.pending_restart = None;
    }
}

#[cfg(test)]
mod tests;
