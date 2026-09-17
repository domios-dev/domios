use super::*;

/// The payload an Orchestrator meant to hand its Planner: backticks, `$`,
/// both quote kinds, non-ASCII punctuation and newlines.
const INTENDED: &str = "Goal: restore the `plan handoff` path so a Planner receives the goal byte-exact.\nRules: never touch $CARGO_TARGET_DIR, keep `build.ps1 test-one` as the only gate.\nDecision: \"early refusal\" vs 'automatic staging' — pick one and say why.\nBudget: $0 extra infra; payload must survive `backticks`, $vars, \"quotes\" and newlines.\n";

/// What argv holds after a shell expands the same text inside a double-quoted
/// word: every backtick span executed and spliced out, the undefined
/// `$CARGO_TARGET_DIR` expanded to nothing. 330 intended bytes arrive as 266,
/// with no error anywhere — the losses happen before `execve`.
const AFTER_SHELL: &str = "Goal: restore the  path so a Planner receives the goal byte-exact.\nRules: never touch , keep  as the only gate.\nDecision: \"early refusal\" vs 'automatic staging' — pick one and say why.\nBudget: $0 extra infra; payload must survive , $vars, \"quotes\" and newlines.";

#[derive(Default)]
struct FakeReceiver {
    accepted: Option<String>,
}

impl FakeReceiver {
    fn accept(&mut self, payload: &str) {
        self.accepted = Some(payload.to_string());
    }
}

/// Positive control the absence assertions below depend on: this harness can
/// see a mangled handoff land. Under the advisory boundary the caller printed a
/// hint and delivered anyway, so the receiver recorded a plausible, shorter,
/// silently wrong payload and the handoff counted as successful.
#[test]
fn quoted_handoff_probe_sees_the_advisory_path_accepting_a_mangled_payload() {
    assert_ne!(AFTER_SHELL, INTENDED, "the fixture must be a real mangle");
    assert!(INTENDED.len() > AFTER_SHELL.len());
    assert!(!INTENDED.contains(": command not found"));
    assert!(
        !AFTER_SHELL.contains(": command not found"),
        "command-substitution stderr never reaches argv, so the existing mangle \
         marker scan cannot see this class"
    );

    assert!(
        inline_refusal(INLINE_SOURCE, AFTER_SHELL, PAD_REMEDY).is_some(),
        "the shape was visible before the fix too — it was only ever advice"
    );

    // The pre-fix caller treated that as advice and delivered.
    let mut receiver = FakeReceiver::default();
    receiver.accept(AFTER_SHELL);
    assert_eq!(receiver.accepted.as_deref(), Some(AFTER_SHELL));
    assert_ne!(receiver.accepted.as_deref(), Some(INTENDED));
}

#[test]
fn quoted_handoff_refuses_the_same_payload_before_any_receiver_sees_it() {
    let refusal = inline_refusal(INLINE_SOURCE, AFTER_SHELL, PAD_REMEDY)
        .expect("an inline authored handoff is refused");
    assert!(refusal.starts_with(REFUSAL_CODE));
    assert!(refusal.contains("codeterm pad path"));
    assert!(refusal.contains("--from-pad"));

    let mut receiver = FakeReceiver::default();
    if inline_refusal(INLINE_SOURCE, AFTER_SHELL, PAD_REMEDY).is_none() {
        receiver.accept(AFTER_SHELL);
    }
    assert_eq!(
        receiver.accepted, None,
        "a refused handoff never transports"
    );
}

/// Staging is the only transport that keeps the intended bytes, and a staged
/// source is never refused for its shape.
#[test]
fn quoted_handoff_staged_sources_deliver_the_intended_bytes() {
    for source in ["--from-pad", "--from-file", "--stdin", "--task-file"] {
        assert!(
            inline_refusal(source, INTENDED, PAD_REMEDY).is_none(),
            "{source} bypasses the shell and must not be refused"
        );
    }
    let mut receiver = FakeReceiver::default();
    receiver.accept(INTENDED);
    assert_eq!(receiver.accepted.as_deref(), Some(INTENDED));
}

#[test]
fn quoted_handoff_keeps_scalar_arguments_inline() {
    for scalar in [
        "run tests",
        "status?",
        "Lifecycle milestone appended",
        "git status",
        "PR #28 — looks fine",
    ] {
        assert!(inline_refusal(INLINE_SOURCE, scalar, PAD_REMEDY).is_none());
    }
    assert!(inline_refusal(INLINE_SOURCE, "", PAD_REMEDY).is_none());
    assert!(inline_refusal(INLINE_SOURCE, &"y".repeat(INLINE_ARGV_BUDGET), PAD_REMEDY).is_none());
    assert!(inline_refusal(
        INLINE_SOURCE,
        &"y".repeat(INLINE_ARGV_BUDGET + 1),
        PAD_REMEDY
    )
    .is_some());
}

/// A quoted single-line payload is refused for length, and one with spaces and
/// quotes reads the same to the boundary — shape decides, never content, since
/// the shell-active characters are the ones already missing.
#[test]
fn quoted_handoff_boundary_reads_shape_not_surviving_characters() {
    let quoted = format!("Goal: \"{}\" and 'more'", "x".repeat(INLINE_ARGV_BUDGET));
    let plain = "x".repeat(quoted.chars().count());
    assert!(inline_refusal(INLINE_SOURCE, &quoted, PAD_REMEDY).is_some());
    assert!(inline_refusal(INLINE_SOURCE, &plain, PAD_REMEDY).is_some());

    let two_lines = "STATUS: in_review\nCHANGES: cli/src/cmd/quoted_handoff.rs";
    assert!(two_lines.chars().count() < INLINE_ARGV_BUDGET);
    assert!(
        inline_refusal(INLINE_SOURCE, two_lines, PAD_REMEDY).is_some(),
        "a newline in argv is authored prose regardless of length"
    );
}

#[test]
fn quoted_handoff_budget_counts_characters_not_bytes() {
    let body = "и".repeat(INLINE_ARGV_BUDGET);
    assert_eq!(body.chars().count(), INLINE_ARGV_BUDGET);
    assert!(body.len() > INLINE_ARGV_BUDGET);
    assert!(inline_refusal(INLINE_SOURCE, &body, PAD_REMEDY).is_none());
}

#[test]
fn quoted_handoff_refusal_names_the_remedy_its_surface_actually_has() {
    let spawn = inline_refusal(INLINE_SOURCE, INTENDED, SPAWN_REMEDY).expect("refused");
    assert!(spawn.contains("--task-file"));
    assert!(!spawn.contains("--from-pad"));

    let pad = inline_refusal(INLINE_SOURCE, INTENDED, PAD_REMEDY).expect("refused");
    assert!(pad.contains("--from-pad"));
    assert!(!pad.contains("--task-file"));
}

#[test]
fn quoted_handoff_refusal_shape_label_distinguishes_the_two_triggers() {
    let multiline = inline_refusal(INLINE_SOURCE, "one\ntwo", PAD_REMEDY).expect("refused");
    assert!(multiline.contains("multiline"));
    let long = inline_refusal(
        INLINE_SOURCE,
        &"z".repeat(INLINE_ARGV_BUDGET + 1),
        PAD_REMEDY,
    )
    .expect("refused");
    assert!(long.contains(&format!("over {INLINE_ARGV_BUDGET} characters")));
}

#[test]
fn quoted_handoff_authored_predicate_has_one_definition() {
    assert!(is_authored_payload("one\ntwo"));
    assert!(is_authored_payload(&"x".repeat(INLINE_ARGV_BUDGET + 1)));
    assert!(!is_authored_payload(&"x".repeat(INLINE_ARGV_BUDGET)));
    assert!(!is_authored_payload("git status"));
    assert_eq!(
        is_authored_payload(AFTER_SHELL),
        crate::cmd::send_text::is_task_shaped_payload(AFTER_SHELL),
        "the raw-shell guard and the handoff guard must share one boundary"
    );
}
