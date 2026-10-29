use codeterm_neighbors::{
    supervisor::{reconnect_backoff_with, retryable, RetryClock},
    Error, Result,
};
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct Rearm {
    epoch: AtomicU64,
    progress: AtomicU64,
    changed: tokio::sync::Notify,
}

impl Rearm {
    pub fn signal(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
    }

    pub fn success(&self) {
        self.progress.fetch_add(1, Ordering::AcqRel);
    }

    async fn after(&self, epoch: u64) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.epoch.load(Ordering::Acquire) != epoch {
                return;
            }
            changed.await;
        }
    }
}

pub(super) async fn run<T, F, A, S, C>(
    rearm: &Rearm,
    clock: &dyn RetryClock,
    mut attempt: A,
    stopped: C,
) -> Result<T>
where
    F: Future<Output = Result<T>>,
    A: FnMut() -> F,
    S: Future<Output = ()>,
    C: Fn() -> S,
{
    let mut failures: usize = 0;
    loop {
        let epoch = rearm.epoch.load(Ordering::Acquire);
        let progress = rearm.progress.load(Ordering::Acquire);
        let result = tokio::select! {
            biased;
            _ = stopped() => return Err(Error::Revoked),
            result = attempt() => result,
        };
        let error = match result {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if rearm.progress.load(Ordering::Acquire) != progress {
            failures = 0;
        }
        tokio::select! {
            biased;
            _ = stopped() => return Err(Error::Revoked),
            _ = async {
                if retryable(error) {
                    failures = failures.saturating_add(1);
                    reconnect_backoff_with(clock, failures).await;
                } else {
                    rearm.after(epoch).await;
                }
            } => {},
        }
    }
}

#[cfg(any(test, not(feature = "test-support")))]
pub(super) fn startup_error(message: &str) -> Error {
    for error in [
        Error::Locked,
        Error::PermissionDenied,
        Error::Revoked,
        Error::Disabled,
        Error::SessionRevoked,
        Error::NotReceiver,
        Error::ReceiverElsewhere,
        Error::ReceivingElsewhere,
        Error::ReceiverNotChosen,
        Error::ReceiverStatusUnknown,
        Error::ReceiversPresent,
        Error::LocalKeyUnavailable,
        Error::SecretStore,
        Error::Refused,
        Error::InstallationUnverified,
    ] {
        if message == error.to_string() {
            return error;
        }
    }
    for (code, error) in [
        ("receiver_not_chosen:", Error::ReceiverNotChosen),
        ("receiver_status_unknown:", Error::ReceiverStatusUnknown),
        ("receivers_present:", Error::ReceiversPresent),
    ] {
        if message.starts_with(code) {
            return error;
        }
    }
    if message.ends_with("(gRPC code 7)") {
        return Error::PermissionDenied;
    }
    if message.starts_with("Sign in ")
        || message == "Neighbor account changed"
        || message == "Neighbor listener changed"
    {
        return Error::Revoked;
    }
    Error::Transport
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) mod test_clock;
