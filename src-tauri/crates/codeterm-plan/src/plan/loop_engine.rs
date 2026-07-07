use super::types::{LoopSpec, LoopStatus};

/// Consecutive no-progress iterations before a deviation directive fires.
pub const STAGNATION_THRESHOLD: u32 = 2;

const MUTATION_ANGLES: [&str; 4] = [
    "try a different decomposition",
    "attack from the opposite end",
    "question a load-bearing assumption",
    "narrow to one sub-case and solve that first",
];

pub fn can_advance(l: &LoopSpec) -> bool {
    l.status == LoopStatus::Running && l.count < l.budget
}

pub fn advance(l: &mut LoopSpec) {
    l.count += 1;
    if l.count >= l.budget {
        l.status = LoopStatus::Exhausted;
    }
}

/// Update the progress/stagnation signal for one iteration. Progress resets and
/// clears any deviation; sustained no-progress (>= K) arms a goal-anchored nudge.
pub fn record_iteration(l: &mut LoopSpec, made_progress: bool, goal_vector: &str) {
    if made_progress {
        l.stagnation = 0;
        l.deviation = None;
    } else {
        l.stagnation += 1;
        if l.stagnation >= STAGNATION_THRESHOLD {
            l.deviation = Some(goal_anchored_directive(l, goal_vector));
        }
    }
}

/// Deterministic deviation directive: rotates the mutation angle by stagnation
/// depth and embeds target + goal so it changes the ANGLE, never the goal.
pub fn goal_anchored_directive(l: &LoopSpec, goal_vector: &str) -> String {
    let idx = (l.stagnation.saturating_sub(STAGNATION_THRESHOLD)) as usize % MUTATION_ANGLES.len();
    let angle = MUTATION_ANGLES[idx];
    let goal = if goal_vector.trim().is_empty() {
        "the plan goal".to_string()
    } else {
        format!("the goal \"{goal_vector}\"")
    };
    format!(
        "Deviation (stagnation {}): {angle}. Keep serving target \"{}\" toward {goal} — change the angle, not the goal.",
        l.stagnation, l.target
    )
}

pub fn stop_for(l: &mut LoopSpec) {
    l.status = LoopStatus::Done;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::types::{LoopSpec, LoopStatus};

    fn running(id: &str) -> LoopSpec {
        LoopSpec {
            id: id.into(),
            target: "harden the parser".into(),
            budget: 0,
            count: 0,
            stop_when: None,
            status: LoopStatus::Running,
            stagnation: 0,
            deviation: None,
            last_iteration_at: None,
        }
    }

    #[test]
    fn loop_advances_until_budget() {
        let mut l = LoopSpec {
            budget: 2,
            ..running("lp")
        };
        assert!(can_advance(&l));
        advance(&mut l);
        assert_eq!(l.count, 1);
        advance(&mut l);
        assert_eq!(l.count, 2);
        assert!(!can_advance(&l));
        assert_eq!(l.status, LoopStatus::Exhausted);
    }

    #[test]
    fn loop_stops_on_signal() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        stop_for(&mut l);
        assert_eq!(l.status, LoopStatus::Done);
        assert!(!can_advance(&l));
    }

    // (a) K consecutive no-progress iters trip stagnation and yield a deviation AFTER K.
    #[test]
    fn stagnation_deviates_only_after_k() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        record_iteration(&mut l, false, "ship the loop");
        assert_eq!(l.stagnation, 1);
        assert!(l.deviation.is_none(), "no deviation before K");
        record_iteration(&mut l, false, "ship the loop");
        assert_eq!(l.stagnation, 2);
        assert!(l.deviation.is_some(), "deviation at K");
    }

    // (b) a progressing loop never deviates.
    #[test]
    fn progressing_loop_never_deviates() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        for _ in 0..5 {
            record_iteration(&mut l, true, "ship the loop");
            assert_eq!(l.stagnation, 0);
            assert!(l.deviation.is_none());
        }
    }

    // (c) directive references the goal and the target.
    #[test]
    fn directive_references_goal_and_target() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        record_iteration(&mut l, false, "ship the loop");
        record_iteration(&mut l, false, "ship the loop");
        let d = l.deviation.unwrap();
        assert!(d.contains("harden the parser"), "references target: {d}");
        assert!(d.contains("ship the loop"), "references goal: {d}");
    }

    // (d) progress after stagnation resets stagnation and clears deviation.
    #[test]
    fn progress_resets_after_stagnation() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        record_iteration(&mut l, false, "g");
        record_iteration(&mut l, false, "g");
        assert!(l.deviation.is_some());
        record_iteration(&mut l, true, "g");
        assert_eq!(l.stagnation, 0);
        assert!(l.deviation.is_none());
    }

    // (e) budget guard still bounds it: can_advance is false at/after budget.
    #[test]
    fn budget_guard_bounds_iteration() {
        let mut l = LoopSpec {
            budget: 1,
            ..running("lp")
        };
        assert!(can_advance(&l));
        advance(&mut l);
        assert!(!can_advance(&l), "exhausted loop cannot advance");
        let mut s = LoopSpec {
            budget: 10,
            stagnation: 5,
            ..running("lp2")
        };
        stop_for(&mut s);
        assert!(!can_advance(&s));
    }

    // directive rotates its angle deterministically as stagnation deepens.
    #[test]
    fn directive_rotates_angle() {
        let mut l = LoopSpec {
            budget: 10,
            ..running("lp")
        };
        let mut seen = vec![];
        for _ in 0..MUTATION_ANGLES.len() + 1 {
            record_iteration(&mut l, false, "g");
            if let Some(d) = &l.deviation {
                seen.push(d.clone());
            }
        }
        // distinct angles appear as stagnation deepens (rotation, not a constant)
        let distinct: std::collections::HashSet<&String> = seen.iter().collect();
        assert!(distinct.len() >= 2, "angles should rotate: {seen:?}");
    }
}
