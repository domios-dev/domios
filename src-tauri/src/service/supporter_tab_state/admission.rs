use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::ObservationTrigger;

const FRAME_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Admission {
    session: String,
    episode: u64,
    frames: HashSet<u64>,
    events: HashMap<&'static str, u64>,
    last_frame_at: Option<Instant>,
}

impl Admission {
    fn claim(
        &mut self,
        session: &str,
        episode: u64,
        frame: u64,
        event: Option<&'static str>,
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
            self.frames.clear();
            self.events.clear();
        }
        if let Some(event) = event {
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
        self.frames.insert(frame);
        self.last_frame_at = Some(now);
        true
    }
}

static ADMISSIONS: LazyLock<Mutex<HashMap<String, Admission>>> = LazyLock::new(Default::default);

pub(super) fn claim(tab: &str, session: &str, frame: u64, trigger: ObservationTrigger) -> bool {
    let event = match trigger {
        ObservationTrigger::SpawnReady => Some("ready"),
        ObservationTrigger::SpawnDeadline => Some("deadline"),
        ObservationTrigger::StartupEscalation => Some("escalation"),
        ObservationTrigger::TabExit => Some("exit"),
        _ => None,
    };
    let episode = super::state::episode(tab, session);
    ADMISSIONS.lock().entry(tab.into()).or_default().claim(
        session,
        episode,
        frame,
        event,
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
    fn unchanged_frames_make_one_call_despite_output_and_failures() {
        let mut admission = Admission::default();
        let start = Instant::now();
        let calls = (0..900)
            .filter(|second| {
                admission.claim("launch", 1, 42, None, start + Duration::from_secs(*second))
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
                    None,
                    start + Duration::from_secs(*second),
                )
            })
            .count();
        assert_eq!(calls, 12);
        assert!(admission.claim("launch", 1, 59, None, start + Duration::from_secs(60)));
        assert!(!admission.claim("launch", 1, 59, None, start + Duration::from_secs(90)));
    }

    #[test]
    fn lifecycle_events_and_new_launches_can_observe_an_unchanged_frame_once() {
        let mut admission = Admission::default();
        let now = Instant::now();
        assert!(admission.claim("launch", 1, 42, None, now));
        assert!(admission.claim("launch", 1, 42, Some("deadline"), now));
        assert!(!admission.claim("launch", 1, 42, Some("deadline"), now));
        assert!(admission.claim("launch", 1, 42, Some("exit"), now));
        assert!(admission.claim("next-launch", 1, 42, None, now));
    }

    #[test]
    fn returning_to_an_observed_frame_in_the_same_episode_is_suppressed() {
        let mut admission = Admission::default();
        let now = Instant::now();
        assert!(admission.claim("launch", 1, 42, None, now));
        assert!(admission.claim("launch", 1, 43, None, now + FRAME_INTERVAL));
        assert!(!admission.claim("launch", 1, 42, None, now + FRAME_INTERVAL * 2));
        assert!(admission.claim("launch", 2, 42, None, now + FRAME_INTERVAL * 2));
    }
}
