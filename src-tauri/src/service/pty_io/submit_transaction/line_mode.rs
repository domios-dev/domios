use crate::agent::provider::ComposerCursorPosition;

pub(super) fn submit_policy(
    track_exact: bool,
    composer: Option<ComposerCursorPosition>,
) -> (bool, usize) {
    match composer {
        Some(ComposerCursorPosition::PromptLine) => (true, 1),
        _ => (track_exact, super::MAX_SUBMIT_KEY_WRITES),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompt_line_uses_exact_evidence_and_never_repeats_empty_submit() {
        assert_eq!(
            submit_policy(false, Some(ComposerCursorPosition::PromptLine)),
            (true, 1)
        );
        for composer in [
            None,
            Some(ComposerCursorPosition::BottomRegion),
            Some(ComposerCursorPosition::VisibleAnywhere),
        ] {
            assert_eq!(
                submit_policy(false, composer),
                (false, super::super::MAX_SUBMIT_KEY_WRITES)
            );
            assert_eq!(
                submit_policy(true, composer),
                (true, super::super::MAX_SUBMIT_KEY_WRITES)
            );
        }
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::super::*;
    use std::cell::Cell;

    struct LineTransport {
        pastes: Cell<usize>,
        keys: Cell<usize>,
        exact: Cell<bool>,
        append_after_key: bool,
    }
    impl SubmitTransport for LineTransport {
        fn observe(&self) -> Option<Observation> {
            None
        }
        fn draft_evidence_count(&self, _: &str) -> usize {
            0
        }
        fn generation(&self) -> u64 {
            0
        }
        fn wait_change(&self, _: u64, _: Instant) -> bool {
            false
        }
        fn turn_started(&self) -> bool {
            true
        }
        fn exact_user_turn_present(&self, _: &str) -> bool {
            self.exact.get()
        }
        fn max_submit_keys(&self) -> usize {
            1
        }
        fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
            self.pastes.set(self.pastes.get() + 1);
            Ok(text.len())
        }
        fn write_submit_key(&self, _: &[u8], _: Instant) -> Result<(), InjectionError> {
            self.keys.set(self.keys.get() + 1);
            Ok(())
        }
        fn wait_for_deferred_submit_ready(&self, _: Instant) -> DeferredSubmitWait {
            if self.append_after_key {
                self.exact.set(true);
                DeferredSubmitWait::Woken
            } else {
                DeferredSubmitWait::Deadline
            }
        }
    }
    #[test]
    fn prompt_line_exact_append_confirms_without_repeating_a_key_or_payload() {
        for append in [true, false] {
            let transport = LineTransport {
                pastes: Cell::new(0),
                keys: Cell::new(0),
                exact: Cell::new(false),
                append_after_key: append,
            };
            let result = runner::run(
                &transport,
                "opaque payload",
                true,
                super::super::tests::tui(),
                super::super::tests::opts(),
            )
            .unwrap();
            assert_eq!(transport.pastes.get(), 1);
            assert_eq!(transport.keys.get(), 1);
            assert_eq!(result.submit_observed, append);
            assert_eq!(result.submit_anomaly, !append);
        }
    }
}
