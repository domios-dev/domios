use super::types::{LoopSpec, LoopStatus};

pub fn can_advance(l: &LoopSpec) -> bool {
    l.status == LoopStatus::Running && l.count < l.budget
}

pub fn advance(l: &mut LoopSpec) {
    l.count += 1;
    if l.count >= l.budget {
        l.status = LoopStatus::Exhausted;
    }
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
            target: "x".into(),
            stages: vec![],
            budget: 0,
            count: 0,
            stop_when: None,
            status: LoopStatus::Running,
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
}
