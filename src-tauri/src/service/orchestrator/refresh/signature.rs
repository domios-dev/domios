//! Structural fingerprint for "compaction has settled" (S3). We match the SHAPE
//! of the post-compaction transcript — a summary block fenced by a horizontal
//! rule run — never vendor copy, which changes without notice. Combined by the
//! driver with a parked tab status; on timeout S3 proceeds regardless so an
//! imperfect fingerprint can never strand the orchestrator.

/// Rule glyphs a TUI uses to fence a compaction summary block.
const RULE_GLYPHS: [char; 4] = ['─', '═', '━', '—'];

/// Minimum run of rule glyphs on one line to count as a summary divider. Short
/// runs appear in ordinary output (tables, spinners); a long unbroken run is the
/// structural tell of a section fence.
const MIN_RULE_RUN: usize = 8;

/// Longest unbroken run of rule glyphs anywhere in `text`.
fn longest_rule_run(text: &str) -> usize {
    let mut best = 0usize;
    let mut cur = 0usize;
    for ch in text.chars() {
        if RULE_GLYPHS.contains(&ch) {
            cur += 1;
            best = best.max(cur);
        } else if !ch.is_whitespace() || ch == '\n' {
            cur = 0;
        }
    }
    best
}

/// Whether `screen_text` carries the structural signature of a settled
/// compaction: a divider-fenced block. Providers render this differently in
/// copy but share the fenced-summary shape.
pub fn compaction_signature(screen_text: &str) -> bool {
    longest_rule_run(screen_text) >= MIN_RULE_RUN
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures capture the SHAPE (divider fence around a collapsed summary), with
    // deliberately generic wording so the detector never keys off vendor copy.
    const CLAUDE_COMPACT_FIXTURE: &str = "\
> /compact
────────────────────────────────────────
 Conversation summarized. Prior context condensed.
 Key state carried forward below.
────────────────────────────────────────
> ";

    const CODEX_COMPACT_FIXTURE: &str = "\
══════════════════════════════════════
  context compacted — resuming
══════════════════════════════════════
";

    const ORDINARY_OUTPUT: &str = "\
running tests...
test result: ok. 12 passed; 0 failed
> ";

    #[test]
    fn detects_claude_style_fence() {
        assert!(compaction_signature(CLAUDE_COMPACT_FIXTURE));
    }

    #[test]
    fn detects_codex_style_fence() {
        assert!(compaction_signature(CODEX_COMPACT_FIXTURE));
    }

    #[test]
    fn ordinary_output_is_not_a_false_positive() {
        assert!(!compaction_signature(ORDINARY_OUTPUT));
    }

    #[test]
    fn short_rule_runs_do_not_trigger() {
        // A three-dash table separator must not read as a compaction fence.
        assert!(!compaction_signature("col ─── col\nval ─── val"));
    }

    #[test]
    fn run_length_counts_across_glyph_families() {
        assert_eq!(longest_rule_run("──══━━—"), 7);
        assert!(longest_rule_run(CLAUDE_COMPACT_FIXTURE) >= MIN_RULE_RUN);
    }
}
