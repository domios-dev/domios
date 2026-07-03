// Structural (not prose-semantic) vagueness check for task test plans, shared by
// the quality doctor and the QA-claim banner. Consistent with doctor philosophy:
// length + command-token only, never a judgment of the prose itself.

// Minimum length below which a test plan reads as an intention, not a scenario.
pub const TEST_PLAN_MIN_CHARS: usize = 120;

// A test plan naming none of these has no runnable oracle.
pub const TEST_PLAN_COMMAND_TOKENS: [&str; 6] = ["`", "npx", "cargo", "codeterm", "bash", "curl"];

// Prepended to the QA-claim output when the claimed task's test plan is vague.
pub const TEST_PLAN_VAGUE_BANNER: &str =
    "TEST PLAN IS VAGUE — bounce to the orchestrator for a concrete plan before verifying.";

pub fn test_plan_is_vague(test_plan: &str) -> bool {
    let trimmed = test_plan.trim();
    trimmed.chars().count() < TEST_PLAN_MIN_CHARS
        || !TEST_PLAN_COMMAND_TOKENS
            .iter()
            .any(|token| trimmed.contains(token))
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
}
