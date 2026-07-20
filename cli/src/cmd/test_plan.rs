// Minimum length below which a test plan reads as an intention, not a scenario.
pub const TEST_PLAN_MIN_CHARS: usize = 120;

// A test plan naming none of these has no runnable oracle.
pub const TEST_PLAN_COMMAND_TOKENS: [&str; 6] = ["`", "npx", "cargo", "codeterm", "bash", "curl"];

// Prepended to the QA-claim output when the claimed task's test plan is vague.
pub const TEST_PLAN_VAGUE_BANNER: &str =
    "TEST PLAN IS VAGUE — bounce to the orchestrator for a concrete plan before verifying.";

/// Names which vagueness criterion failed, so callers can tell the author exactly
/// what to fix instead of a bare "vague" verdict. `None` when the plan passes both.
pub fn test_plan_vague_reason(test_plan: &str) -> Option<String> {
    let trimmed = test_plan.trim();
    let mut reasons = Vec::new();
    let char_count = trimmed.chars().count();
    if char_count < TEST_PLAN_MIN_CHARS {
        reasons.push(format!(
            "too short ({char_count} chars, need at least {TEST_PLAN_MIN_CHARS})"
        ));
    }
    if !TEST_PLAN_COMMAND_TOKENS
        .iter()
        .any(|token| trimmed.contains(token))
    {
        reasons.push(format!(
            "names no runnable command (expected one of: {})",
            TEST_PLAN_COMMAND_TOKENS.join(", ")
        ));
    }
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

pub fn test_plan_is_vague(test_plan: &str) -> bool {
    test_plan_vague_reason(test_plan).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A concrete, executable plan: long enough AND names a runnable command.
    const GOOD_PLAN: &str = "Rust unit: cover the vague-predicate matrix (under-length, no-command, good). \
Run `cargo test test_plan_vague` and assert exactly one test_plan_vague finding on the seeded plan.";

    #[test]
    fn under_length_plan_is_vague() {
        assert!(test_plan_is_vague("component tests; manual check"));
    }

    #[test]
    fn commandless_plan_is_vague_even_when_long() {
        let wordy = "We will thoroughly verify the behavior by carefully inspecting the \
resulting output and confirming that everything looks correct across all the relevant surfaces.";
        assert!(wordy.chars().count() >= TEST_PLAN_MIN_CHARS);
        assert!(test_plan_is_vague(wordy));
    }

    #[test]
    fn good_plan_with_backticked_command_is_not_vague() {
        assert!(GOOD_PLAN.chars().count() >= TEST_PLAN_MIN_CHARS);
        assert!(!test_plan_is_vague(GOOD_PLAN));
    }

    #[test]
    fn command_token_alone_without_length_is_still_vague() {
        assert!(test_plan_is_vague("`cargo test`"));
    }

    #[test]
    fn reason_names_short_length_criterion() {
        let reason = test_plan_vague_reason("`cargo test`").unwrap();
        assert!(reason.contains("too short"), "{reason}");
        assert!(!reason.contains("no runnable command"), "{reason}");
    }

    #[test]
    fn reason_names_missing_command_criterion_and_lists_tokens() {
        let wordy = "We will thoroughly verify the behavior by carefully inspecting the \
resulting output and confirming that everything looks correct across all the relevant surfaces.";
        let reason = test_plan_vague_reason(wordy).unwrap();
        assert!(reason.contains("names no runnable command"), "{reason}");
        for token in TEST_PLAN_COMMAND_TOKENS {
            assert!(reason.contains(token), "{reason}");
        }
        assert!(!reason.contains("too short"), "{reason}");
    }

    #[test]
    fn reason_names_both_criteria_when_both_fail() {
        let reason = test_plan_vague_reason("component tests; manual check").unwrap();
        assert!(reason.contains("too short"), "{reason}");
        assert!(reason.contains("names no runnable command"), "{reason}");
    }

    #[test]
    fn good_plan_has_no_vague_reason() {
        assert!(test_plan_vague_reason(GOOD_PLAN).is_none());
    }
}
