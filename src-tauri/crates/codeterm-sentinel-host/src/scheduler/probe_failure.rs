use crate::probe::process::{ProbeFailure, ResourceKind};

const LIMIT: u8 = 3;
const PREFIX: &str = "probe_capture_failure; consecutive=";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Ordinary,
    Skip(String),
    Stop,
    StopResources,
}

pub(super) fn decide(previous: Option<&str>, failure: &ProbeFailure) -> Decision {
    if failure.primary == "probe_owner_capacity" {
        return Decision::StopResources;
    }
    if failure.primary == "probe_timeout" {
        return Decision::Skip(failure.reason());
    }
    let capture = matches!(
        failure.primary,
        "probe_capture_or_completion_failed"
            | "probe_capture_read_failed"
            | "probe_capture_failed"
            | "probe_wait_failed"
            | "probe_capture_eof_unconfirmed"
    );
    let capture_only = failure
        .unconfirmed
        .iter()
        .all(|resource| matches!(resource, ResourceKind::Capture | ResourceKind::OverlappedIo));
    if !capture
        || !failure.processes_confirmed
        || !capture_only
        || (failure.is_unconfirmed() && failure.unconfirmed.is_empty())
    {
        return if failure.is_unconfirmed() {
            Decision::StopResources
        } else {
            Decision::Ordinary
        };
    }
    let previous = previous
        .and_then(|reason| reason.strip_prefix(PREFIX))
        .and_then(|rest| rest.split(';').next())
        .and_then(|count| count.parse::<u8>().ok())
        .unwrap_or(0);
    let count = previous.saturating_add(1);
    if count >= LIMIT {
        Decision::Stop
    } else {
        Decision::Skip(format!("{PREFIX}{count}; {}", failure.reason()))
    }
}

#[cfg(test)]
mod tests;
