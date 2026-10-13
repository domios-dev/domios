use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Completions {
    sequence: u64,
    turns: VecDeque<(String, String)>,
}
impl Completions {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn record(&mut self, thread: &str, turn: &str) {
        self.sequence = self.sequence.saturating_add(1);
        if !self
            .turns
            .iter()
            .any(|(known_thread, known_turn)| known_thread == thread && known_turn == turn)
        {
            self.turns.push_back((thread.into(), turn.into()));
            if self.turns.len() > 128 {
                self.turns.pop_front();
            }
        }
    }
    pub fn permits_receipt(
        &self,
        thread: &str,
        turn: &str,
        current: Option<&str>,
        submitted: Option<&str>,
        before: u64,
    ) -> bool {
        if self
            .turns
            .iter()
            .any(|(known_thread, known_turn)| known_thread == thread && known_turn == turn)
        {
            return false;
        }
        match current {
            Some(current) => current == turn || Some(current) == submitted,
            None => self.sequence == before,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_or_newer_goal_turn_cannot_be_resurrected_by_a_late_receipt() {
        let mut completions = Completions::default();
        assert!(completions.permits_receipt("thread", "new", None, None, 0));
        completions.record("thread", "old");
        assert!(!completions.permits_receipt("thread", "old", None, None, 0));
        assert!(!completions.permits_receipt("thread", "old", Some("goal-next"), None, 0));
        assert!(completions.permits_receipt("thread", "goal-next", Some("goal-next"), None, 0));
    }
    #[test]
    fn bounded_completion_history_retains_the_receipt_barrier_after_eviction() {
        let mut completions = Completions::default();
        for number in 0..200 {
            completions.record("thread", &number.to_string());
        }
        assert_eq!(completions.turns.len(), 128);
        assert!(!completions.permits_receipt("thread", "0", None, None, 0));
        assert!(completions.permits_receipt("thread", "fresh", None, None, completions.sequence()));
    }
}
