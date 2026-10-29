use crate::host_ports::native_client::{ExactServerShutdownRefusal, ExactServerShutdownResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuitOrigin {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExitGate {
    Idle,
    Requested,
    Uncertain,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuitFeedback {
    Waiting,
    Refused,
    Uncertain,
    OfferCloseAfterRefusal,
    OfferCloseAfterUncertainty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuitStart {
    RequestServerShutdown,
    CloseClientOnly,
    Busy(QuitFeedback),
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShutdownOutcome {
    Confirmed,
    ServerUnreachable,
    RefusedBeforeDispatch,
    UncertainAfterDispatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuitResolution {
    ExitClient,
    CloseClientOnly,
    Feedback(QuitFeedback),
}

pub(super) fn plan_quit_start(origin: QuitOrigin, gate: ExitGate, attached: bool) -> QuitStart {
    match (gate, origin) {
        (ExitGate::Idle, _) if attached => QuitStart::RequestServerShutdown,
        (ExitGate::Idle, QuitOrigin::Local) => QuitStart::CloseClientOnly,
        (ExitGate::Idle, QuitOrigin::Remote) => QuitStart::Unavailable,
        (ExitGate::Uncertain, QuitOrigin::Local) => {
            QuitStart::Busy(QuitFeedback::OfferCloseAfterUncertainty)
        }
        (ExitGate::Uncertain, QuitOrigin::Remote) => QuitStart::Busy(QuitFeedback::Uncertain),
        (ExitGate::Requested | ExitGate::Confirmed, _) => QuitStart::Busy(QuitFeedback::Waiting),
    }
}

pub(super) fn classify_shutdown(result: &ExactServerShutdownResult) -> ShutdownOutcome {
    match result {
        ExactServerShutdownResult::Confirmed { .. } => ShutdownOutcome::Confirmed,
        ExactServerShutdownResult::RefusedBeforeDispatch(
            ExactServerShutdownRefusal::ApiAttachmentUnavailable
            | ExactServerShutdownRefusal::NativeHostUnavailable,
        ) => ShutdownOutcome::ServerUnreachable,
        ExactServerShutdownResult::RefusedBeforeDispatch(_) => {
            ShutdownOutcome::RefusedBeforeDispatch
        }
        ExactServerShutdownResult::UncertainAfterDispatch(_) => {
            ShutdownOutcome::UncertainAfterDispatch
        }
    }
}

pub(super) fn plan_quit_result(origin: QuitOrigin, outcome: ShutdownOutcome) -> QuitResolution {
    match (outcome, origin) {
        (ShutdownOutcome::Confirmed, _) => QuitResolution::ExitClient,
        (ShutdownOutcome::ServerUnreachable, QuitOrigin::Local) => QuitResolution::CloseClientOnly,
        (ShutdownOutcome::RefusedBeforeDispatch, QuitOrigin::Local) => {
            QuitResolution::Feedback(QuitFeedback::OfferCloseAfterRefusal)
        }
        (ShutdownOutcome::UncertainAfterDispatch, QuitOrigin::Local) => {
            QuitResolution::Feedback(QuitFeedback::OfferCloseAfterUncertainty)
        }
        (
            ShutdownOutcome::ServerUnreachable | ShutdownOutcome::RefusedBeforeDispatch,
            QuitOrigin::Remote,
        ) => QuitResolution::Feedback(QuitFeedback::Refused),
        (ShutdownOutcome::UncertainAfterDispatch, QuitOrigin::Remote) => {
            QuitResolution::Feedback(QuitFeedback::Uncertain)
        }
    }
}

pub(super) fn finish_quit(
    origin: QuitOrigin,
    result: &ExactServerShutdownResult,
    confirm_ticket: impl FnOnce() -> bool,
    resolve: impl FnOnce(QuitResolution),
) {
    let outcome = classify_shutdown(result);
    if outcome == ShutdownOutcome::Confirmed && !confirm_ticket() {
        return;
    }
    resolve(plan_quit_result(origin, outcome));
}

#[cfg(test)]
mod tests;
