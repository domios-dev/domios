//! Pure transition core for the orchestrator refresh cycle (S1..S5). No I/O:
//! every side effect is decided here and executed by the driver, so the whole
//! ordered state machine is table-testable like `pty_io::wake`. Advancing to
//! phase N+1 is gated on VERIFIED completion of phase N — never a bare send.

use std::time::Duration;

/// The five verified refresh steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPhase {
    /// S1 — waiting for the requested HANDOFF stride to land in plan readback.
    HandoffSnapshot,
    /// S2 — compact command (or mode-B fallback) fired on entry; passes through.
    Compact,
    /// S3 — waiting for the compaction transcript signature + parked status.
    CompactSettle,
    /// S4 — re-orient template injected; waiting for delivery confirmation.
    Reorient,
    /// S5 — waiting for the orchestrator's first post-refresh plan-state read.
    VerifyResumption,
}

impl RefreshPhase {
    /// Stable transcript tag for ordering assertions in the drills.
    pub fn tag(self) -> &'static str {
        match self {
            Self::HandoffSnapshot => "s1_handoff",
            Self::Compact => "s2_compact",
            Self::CompactSettle => "s3_settle",
            Self::Reorient => "s4_reorient",
            Self::VerifyResumption => "s5_verify",
        }
    }
}

/// Bounded timeouts for the phases that wait on an external effect. Sourced from
/// `[orchestration.refresh]` config; defaults keep a hung compact from stranding.
#[derive(Debug, Clone, Copy)]
pub struct RefreshTuning {
    pub handoff_timeout: Duration,
    pub settle_timeout: Duration,
    pub reorient_timeout: Duration,
    pub verify_timeout: Duration,
}

impl Default for RefreshTuning {
    fn default() -> Self {
        Self {
            handoff_timeout: Duration::from_secs(60),
            settle_timeout: Duration::from_secs(120),
            reorient_timeout: Duration::from_secs(30),
            verify_timeout: Duration::from_secs(180),
        }
    }
}

/// What the driver observed this tick. All booleans are VERIFIED effects, never
/// bare submits (handoff via plan readback, resumption via CLI telemetry, etc).
#[derive(Debug, Clone, Copy, Default)]
pub struct Observation {
    /// S1 readback: a new handoff element with seq > pre-refresh baseline landed.
    pub handoff_landed: bool,
    /// S3: compaction transcript signature seen AND pane parked (Idle/Waiting).
    pub settle_detected: bool,
    /// S4: `Messenger::deliver` of the re-orient template confirmed submitted.
    pub reorient_delivered: bool,
    /// S5: orchestrator's next observable action (plan-state read / heartbeat).
    pub resumption_observed: bool,
    /// Time spent in the current phase.
    pub elapsed_in_phase: Duration,
    /// S5: the single re-nudge has already been sent.
    pub renudged: bool,
}

/// One tick's decision. `Enter` carries the phase to transition INTO — the
/// driver runs that phase's entry side-effect (send / inject) exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Stay in the current phase; keep polling.
    Hold,
    /// Transition into `phase` and fire its entry side-effect once.
    Enter(RefreshPhase),
    /// S5 dead-resumption: re-inject the re-orient template ONCE, reset the timer.
    Renudge,
    /// Verified resumption — terminal success.
    Complete,
    /// Loud terminal failure — raise notification + planner decision gate.
    Alert,
}

/// The whole ordered state machine as one pure function. Deterministic in
/// `(phase, obs, tuning)` so every branch — happy path, hung-compact→S4, and
/// dead-resumption→re-nudge→alert — is a table row.
pub fn advance(phase: RefreshPhase, obs: &Observation, t: &RefreshTuning) -> Step {
    match phase {
        // S1: never compact without a verified snapshot. Timeout is loud, not silent.
        RefreshPhase::HandoffSnapshot => {
            if obs.handoff_landed {
                Step::Enter(RefreshPhase::Compact)
            } else if obs.elapsed_in_phase >= t.handoff_timeout {
                Step::Alert
            } else {
                Step::Hold
            }
        }
        // S2: compact was fired on entry; deterministically move to settle-watch.
        RefreshPhase::Compact => Step::Enter(RefreshPhase::CompactSettle),
        // S3: proceed on settle OR on timeout — re-orient is safe regardless, and
        // a hung compact must NOT strand the orchestrator.
        RefreshPhase::CompactSettle => {
            if obs.settle_detected || obs.elapsed_in_phase >= t.settle_timeout {
                Step::Enter(RefreshPhase::Reorient)
            } else {
                Step::Hold
            }
        }
        // S4: delivery is wake-confirmer backstopped; proceed on confirmation or
        // timeout so a swallowed submit can't strand us before verification.
        RefreshPhase::Reorient => {
            if obs.reorient_delivered || obs.elapsed_in_phase >= t.reorient_timeout {
                Step::Enter(RefreshPhase::VerifyResumption)
            } else {
                Step::Hold
            }
        }
        // S5: the only forbidden outcome is silence — every branch ends in
        // Complete (verified) or Alert (loud), with exactly one re-nudge between.
        RefreshPhase::VerifyResumption => {
            if obs.resumption_observed {
                Step::Complete
            } else if obs.elapsed_in_phase >= t.verify_timeout {
                if obs.renudged {
                    Step::Alert
                } else {
                    Step::Renudge
                }
            } else {
                Step::Hold
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs() -> Observation {
        Observation::default()
    }

    fn tuning() -> RefreshTuning {
        RefreshTuning::default()
    }

    #[test]
    fn s1_holds_until_handoff_lands_then_compacts() {
        let t = tuning();
        assert_eq!(
            advance(RefreshPhase::HandoffSnapshot, &obs(), &t),
            Step::Hold
        );
        let landed = Observation {
            handoff_landed: true,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::HandoffSnapshot, &landed, &t),
            Step::Enter(RefreshPhase::Compact)
        );
    }

    #[test]
    fn s1_timeout_alerts_never_compacts_without_snapshot() {
        let t = tuning();
        let stale = Observation {
            handoff_landed: false,
            elapsed_in_phase: t.handoff_timeout,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::HandoffSnapshot, &stale, &t),
            Step::Alert
        );
    }

    #[test]
    fn s2_always_passes_through_to_settle() {
        // Compact is fired on entry; the machine never lingers here.
        let t = tuning();
        for o in [
            obs(),
            Observation {
                elapsed_in_phase: Duration::from_secs(999),
                ..obs()
            },
        ] {
            assert_eq!(
                advance(RefreshPhase::Compact, &o, &t),
                Step::Enter(RefreshPhase::CompactSettle)
            );
        }
    }

    #[test]
    fn s3_advances_on_signature() {
        let t = tuning();
        let settled = Observation {
            settle_detected: true,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::CompactSettle, &settled, &t),
            Step::Enter(RefreshPhase::Reorient)
        );
    }

    #[test]
    fn s3_hung_compact_still_reaches_reorient_on_timeout() {
        // The hung-compact failure branch: never stranded, always re-orients.
        let t = tuning();
        assert_eq!(advance(RefreshPhase::CompactSettle, &obs(), &t), Step::Hold);
        let timed_out = Observation {
            settle_detected: false,
            elapsed_in_phase: t.settle_timeout,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::CompactSettle, &timed_out, &t),
            Step::Enter(RefreshPhase::Reorient)
        );
    }

    #[test]
    fn s4_advances_on_delivery_or_timeout() {
        let t = tuning();
        let delivered = Observation {
            reorient_delivered: true,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::Reorient, &delivered, &t),
            Step::Enter(RefreshPhase::VerifyResumption)
        );
        let timed_out = Observation {
            elapsed_in_phase: t.reorient_timeout,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::Reorient, &timed_out, &t),
            Step::Enter(RefreshPhase::VerifyResumption)
        );
    }

    #[test]
    fn s5_completes_on_resumption() {
        let t = tuning();
        let resumed = Observation {
            resumption_observed: true,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::VerifyResumption, &resumed, &t),
            Step::Complete
        );
    }

    #[test]
    fn s5_dead_resumption_renudges_once_then_alerts() {
        // The dead-resumption failure branch: exactly one re-nudge, then a loud alert.
        let t = tuning();
        let first_timeout = Observation {
            resumption_observed: false,
            renudged: false,
            elapsed_in_phase: t.verify_timeout,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::VerifyResumption, &first_timeout, &t),
            Step::Renudge
        );
        let second_timeout = Observation {
            renudged: true,
            ..first_timeout
        };
        assert_eq!(
            advance(RefreshPhase::VerifyResumption, &second_timeout, &t),
            Step::Alert
        );
    }

    #[test]
    fn s5_holds_before_verify_timeout() {
        let t = tuning();
        let waiting = Observation {
            elapsed_in_phase: t.verify_timeout / 2,
            ..obs()
        };
        assert_eq!(
            advance(RefreshPhase::VerifyResumption, &waiting, &t),
            Step::Hold
        );
    }

    #[test]
    fn full_happy_path_is_strictly_ordered() {
        // Walk S1→S5 driving only verified effects; assert the exact phase order.
        let t = tuning();
        let mut phase = RefreshPhase::HandoffSnapshot;
        let mut order = vec![phase.tag()];
        let ticks = [
            Observation {
                handoff_landed: true,
                ..obs()
            },
            obs(), // Compact pass-through
            Observation {
                settle_detected: true,
                ..obs()
            },
            Observation {
                reorient_delivered: true,
                ..obs()
            },
            Observation {
                resumption_observed: true,
                ..obs()
            },
        ];
        for o in ticks {
            match advance(phase, &o, &t) {
                Step::Enter(next) => {
                    phase = next;
                    order.push(phase.tag());
                }
                Step::Complete => {
                    order.push("resumed");
                    break;
                }
                other => panic!("unexpected step {other:?} in phase {phase:?}"),
            }
        }
        assert_eq!(
            order,
            vec![
                "s1_handoff",
                "s2_compact",
                "s3_settle",
                "s4_reorient",
                "s5_verify",
                "resumed",
            ]
        );
    }
}
