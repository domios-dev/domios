use super::*;

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
    let quoted = format!("`\"{}'${}", "x".repeat(INLINE_ARGV_BUDGET), "y");
    let plain = "x".repeat(quoted.chars().count());
    assert!(inline_refusal(INLINE_SOURCE, &quoted, PAD_REMEDY).is_some());
    assert!(inline_refusal(INLINE_SOURCE, &plain, PAD_REMEDY).is_some());

    let two_lines = "q\n\"x'y`$";
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
    let hostile = format!("q\n\"{}'`$", "x".repeat(INLINE_ARGV_BUDGET));
    let spawn = inline_refusal(INLINE_SOURCE, &hostile, SPAWN_REMEDY).expect("refused");
    assert!(spawn.contains("--task-file"));
    assert!(!spawn.contains("--from-pad"));

    let pad = inline_refusal(INLINE_SOURCE, &hostile, PAD_REMEDY).expect("refused");
    assert!(pad.contains("--from-pad"));
    assert!(!pad.contains("--task-file"));
}
