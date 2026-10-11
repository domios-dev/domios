use crate::sentinel::probe::process::{ProbeFailure, ResourceKind};

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
mod tests {
    use super::*;

    #[test]
    fn confirmed_timeout_keeps_watching_but_unknown_resources_remain_fenced() {
        let failure = ProbeFailure::new("probe_timeout");
        assert_eq!(decide(None, &failure), Decision::Ordinary);
        let mut uncertain = failure;
        uncertain.unconfirmed(ResourceKind::ProcessGroup);
        assert_eq!(decide(None, &uncertain), Decision::StopResources);
    }

    #[test]
    fn probe_owner_capacity_stops_for_routed_lifecycle_failure() {
        assert_eq!(
            decide(None, &ProbeFailure::new("probe_owner_capacity")),
            Decision::StopResources
        );
    }

    fn capture() -> ProbeFailure {
        let mut failure = ProbeFailure::new("probe_capture_or_completion_failed");
        failure.processes_confirmed = true;
        failure.unconfirmed(ResourceKind::OverlappedIo);
        failure
    }

    #[test]
    fn windows_capture_mapping_preserves_command_uncertainty_but_skips_probe() {
        let failure = capture();
        assert!(failure.is_unconfirmed());
        assert!(matches!(decide(None, &failure), Decision::Skip(_)));
    }

    #[test]
    fn unconfirmed_child_or_missing_completion_never_downgrades() {
        let mut failure = capture();
        failure.processes_confirmed = false;
        assert_eq!(decide(None, &failure), Decision::StopResources);
        failure.processes_confirmed = true;
        for resource in [
            ResourceKind::Child,
            ResourceKind::Job,
            ResourceKind::ProcessGroup,
        ] {
            let mut failure = failure.clone();
            failure.unconfirmed(resource);
            assert_eq!(decide(None, &failure), Decision::StopResources);
        }
    }

    #[test]
    fn third_consecutive_failure_stops_and_other_outcomes_reset_count() {
        let failure = capture();
        let Decision::Skip(first) = decide(None, &failure) else {
            panic!()
        };
        let Decision::Skip(second) = decide(Some(&first), &failure) else {
            panic!()
        };
        assert_eq!(decide(Some(&second), &failure), Decision::Stop);
        assert_eq!(decide(Some("unchanged"), &failure), decide(None, &failure));
        assert_eq!(
            decide(Some(&second), &ProbeFailure::new("probe_timeout")),
            Decision::Ordinary
        );
    }
}
