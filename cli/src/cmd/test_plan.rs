// Minimum length below which a test plan reads as an intention, not a scenario.
pub const TEST_PLAN_MIN_CHARS: usize = 120;

pub const TEST_PLAN_COMMAND_RUNNERS: [&str; 11] = [
    "npx",
    "cargo",
    "codeterm",
    "bash",
    "curl",
    "npm",
    "build.ps1",
    "build.sh",
    "pnpm",
    "node",
    "./build.sh",
];

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
    if !names_command_invocation(trimmed) {
        reasons.push(format!(
            "names no runnable command (expected one of: {})",
            TEST_PLAN_COMMAND_RUNNERS.join(", ")
        ));
    }
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

fn names_command_invocation(test_plan: &str) -> bool {
    test_plan
        .split(['\n', ';', '`'])
        .map(str::trim_start)
        .map(|segment| {
            segment
                .strip_prefix("$ ")
                .or_else(|| segment.strip_prefix("> "))
                .unwrap_or(segment)
        })
        .any(|segment| {
            if segment.starts_with("bash -c ") || segment.starts_with("bash -lc ") {
                return true;
            }
            TEST_PLAN_COMMAND_RUNNERS.iter().any(|runner| {
                segment
                    .strip_prefix(runner)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            })
        })
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
    fn bash_c_test_plan_is_a_runnable_command() {
        let prefix =
            "The test plan has enough detail to identify the suite and observable result. ";
        assert!(test_plan_vague_reason(&format!(
            "{prefix}`bash -c 'python -m unittest tests.test_cli'`"
        ))
        .is_none());
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
    fn diagnostics_that_speak_vagueness_requires_command_position_not_substrings() {
        let wordy = "We will thoroughly verify the behavior by carefully inspecting the \
resulting output and confirming that everything looks correct across all the relevant surfaces.";
        let reason = test_plan_vague_reason(wordy).unwrap();
        assert!(reason.contains("names no runnable command"), "{reason}");
        for token in TEST_PLAN_COMMAND_RUNNERS {
            assert!(reason.contains(token), "{reason}");
        }
        assert!(!reason.contains("too short"), "{reason}");
    }

    #[test]
    fn diagnostics_that_speak_vagueness_accepts_real_repo_runners() {
        let prefix = "This plan has enough detail to clear the length criterion while it describes the expected observable result. ";
        assert!(test_plan_vague_reason(&format!(
            "{prefix}`build.ps1 test-one app diagnostics_that_speak`"
        ))
        .is_none());
        assert!(
            test_plan_vague_reason(&format!("{prefix}`npm run test:unit -- webForks`")).is_none()
        );
        assert!(test_plan_vague_reason(&format!(
            "{prefix}The frozen-zone grep mentions codeterm-pty, but it is not a command."
        ))
        .is_some());
        assert!(test_plan_vague_reason(&format!(
            "{prefix}A stray ` in prose is not an invocation."
        ))
        .is_some());
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
