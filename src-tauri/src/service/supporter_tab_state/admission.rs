use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::ObservationTrigger;

const FRAME_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Cause {
    Frame,
    FirstOpinion,
    Lifecycle(&'static str),
}

impl From<ObservationTrigger> for Cause {
    fn from(trigger: ObservationTrigger) -> Self {
        match trigger {
            ObservationTrigger::OutputCandidate | ObservationTrigger::ParkedEdge => {
                Self::FirstOpinion
            }
            ObservationTrigger::SpawnReady => Self::Lifecycle("ready"),
            ObservationTrigger::SpawnDeadline => Self::Lifecycle("deadline"),
            ObservationTrigger::StartupEscalation => Self::Lifecycle("escalation"),
            ObservationTrigger::TabExit => Self::Lifecycle("exit"),
            _ => Self::Frame,
        }
    }
}

#[derive(Default)]
struct Admission {
    session: String,
    episode: u64,
    frames: HashSet<u64>,
    events: HashMap<&'static str, u64>,
    last_frame_at: Option<Instant>,
    last_frame: Option<u64>,
    first_opinion_observed: bool,
}

impl Admission {
    fn claim(
        &mut self,
        session: &str,
        episode: u64,
        frame: u64,
        cause: Cause,
        now: Instant,
    ) -> bool {
        if self.session != session {
            *self = Self {
                session: session.into(),
                ..Self::default()
            };
        }
        if self.episode != episode {
            self.episode = episode;
        }
        if self.last_frame == Some(frame) && matches!(cause, Cause::FirstOpinion) {
            self.first_opinion_observed = true;
        }
        if self.last_frame == Some(frame) && !matches!(cause, Cause::Lifecycle(_)) {
            return false;
        }
        if matches!(cause, Cause::FirstOpinion) && !self.first_opinion_observed {
            self.first_opinion_observed = true;
            self.frames.insert(frame);
            self.last_frame = Some(frame);
            self.last_frame_at = Some(now);
            return true;
        }
        if let Cause::Lifecycle(event) = cause {
            if self.events.get(event) == Some(&frame) {
                return false;
            }
            self.events.insert(event, frame);
            self.frames.insert(frame);
            return true;
        }
        if self.frames.contains(&frame)
            || self
                .last_frame_at
                .is_some_and(|last| now.saturating_duration_since(last) < FRAME_INTERVAL)
        {
            return false;
        }
        if self.frames.len() >= 256 {
            self.frames.clear();
        }
        self.frames.insert(frame);
        self.last_frame = Some(frame);
        self.last_frame_at = Some(now);
        true
    }
}

pub(super) fn structurally_decisive(
    trigger: ObservationTrigger,
    state: &str,
    confidence: f64,
    live_state: &str,
    pending_prompt: bool,
) -> bool {
    !pending_prompt
        && state == "working"
        && confidence.is_finite()
        && (confidence >= 0.9 || live_state == "working")
        && matches!(
            trigger,
            ObservationTrigger::OutputCandidate
                | ObservationTrigger::ParkedEdge
                | ObservationTrigger::BusyNoOutput
        )
}

static ADMISSIONS: LazyLock<Mutex<HashMap<String, Admission>>> = LazyLock::new(Default::default);

pub(super) fn claim(tab: &str, session: &str, frame: u64, trigger: ObservationTrigger) -> bool {
    let episode = super::state::episode(tab, session);
    ADMISSIONS.lock().entry(tab.into()).or_default().claim(
        session,
        episode,
        frame,
        trigger.into(),
        Instant::now(),
    )
}

pub(super) fn forget(tab: &str) {
    ADMISSIONS.lock().remove(tab);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confident_working_adapter_suppresses_background_only() {
        assert!(structurally_decisive(
            ObservationTrigger::BusyNoOutput,
            "working",
            0.95,
            "",
            false
        ));
        assert!(structurally_decisive(
            ObservationTrigger::BusyNoOutput,
            "working",
            0.0,
            "working",
            false
        ));
        assert!(!structurally_decisive(
            ObservationTrigger::BusyNoOutput,
            "working",
            0.5,
            "",
            false
        ));
        assert!(!structurally_decisive(
            ObservationTrigger::BusyNoOutput,
            "working",
            0.95,
            "working",
            true
        ));
        assert!(!structurally_decisive(
            ObservationTrigger::SpawnDeadline,
            "working",
            0.95,
            "working",
            false
        ));
        assert!(!structurally_decisive(
            ObservationTrigger::OutputCandidate,
            "unknown",
            1.0,
            "working",
            false
        ));
    }

    #[test]
    fn unchanged_frames_make_one_call_despite_output_and_failures() {
        let mut admission = Admission::default();
        let start = Instant::now();
        let calls = (0..900)
            .filter(|second| {
                admission.claim(
                    "launch",
                    1,
                    42,
                    Cause::Frame,
                    start + Duration::from_secs(*second),
                )
            })
            .count();
        assert_eq!(calls, 1);
    }

    #[test]
    fn changed_frames_are_bounded_and_latest_frame_remains_eligible() {
        let mut admission = Admission::default();
        let start = Instant::now();
        let calls = (0..60)
            .filter(|second| {
                admission.claim(
                    "launch",
                    1,
                    *second,
                    Cause::Frame,
                    start + Duration::from_secs(*second),
                )
            })
            .count();
        assert_eq!(calls, 12);
        assert!(admission.claim(
            "launch",
            1,
            59,
            Cause::Frame,
            start + Duration::from_secs(60)
        ));
        assert!(!admission.claim(
            "launch",
            1,
            59,
            Cause::Frame,
            start + Duration::from_secs(90)
        ));
    }

    #[test]
    fn lifecycle_events_and_new_launches_can_observe_an_unchanged_frame_once() {
        let mut admission = Admission::default();
        let now = Instant::now();
        assert!(admission.claim("launch", 1, 42, Cause::Frame, now));
        assert!(admission.claim("launch", 1, 42, Cause::Lifecycle("deadline"), now));
        assert!(!admission.claim("launch", 1, 42, Cause::Lifecycle("deadline"), now));
        assert!(admission.claim("launch", 1, 42, Cause::Lifecycle("exit"), now));
        assert!(admission.claim("next-launch", 1, 42, Cause::Frame, now));
    }

    #[test]
    fn first_opinion_is_once_per_launch_and_preserves_the_periodic_rate_bound() {
        let mut admission = Admission::default();
        let now = Instant::now();
        assert!(admission.claim("launch", 1, 42, Cause::Frame, now));
        assert!(!admission.claim(
            "launch",
            1,
            42,
            ObservationTrigger::OutputCandidate.into(),
            now
        ));
        for frame in 42..100 {
            assert!(!admission.claim(
                "launch",
                1,
                frame,
                ObservationTrigger::ParkedEdge.into(),
                now
            ));
        }
        assert!(!admission.claim("launch", 2, 43, Cause::FirstOpinion, now));
        assert!(admission.claim("next-launch", 2, 43, Cause::FirstOpinion, now));
        assert!(!admission.claim("next-launch", 2, 43, Cause::Frame, now));
        let calls = (0..60)
            .filter(|second| {
                admission.claim(
                    "next-launch",
                    2,
                    100 + *second,
                    Cause::FirstOpinion,
                    now + Duration::from_secs(*second),
                )
            })
            .count();
        assert_eq!(calls, 11);
    }

    #[test]
    fn returning_to_an_observed_frame_in_the_same_episode_is_suppressed() {
        let mut admission = Admission::default();
        let now = Instant::now();
        assert!(admission.claim("launch", 1, 42, Cause::Frame, now));
        assert!(admission.claim("launch", 1, 43, Cause::Frame, now + FRAME_INTERVAL));
        assert!(!admission.claim("launch", 1, 42, Cause::Frame, now + FRAME_INTERVAL * 2));
        assert!(!admission.claim("launch", 2, 42, Cause::Frame, now + FRAME_INTERVAL * 2));
    }
}
const _SPLIT_METRIC_PROBE: u8 = 0;
