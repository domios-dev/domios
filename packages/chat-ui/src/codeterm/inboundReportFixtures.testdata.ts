/**
 * Real report payloads captured off the wire (pane 02d1339c, plan pl_ghk358).
 * The status line is the leading structured block; everything after it is the
 * free-form message a report card must keep rendering.
 */

/** Verbatim marker payload: `status: Partial` + a 21-line free-form body. */
export const CURIE_PARTIAL_REPORT = [
  "status: Partial",
  "REPORT NOT SENT",
  "readiness report refused: this git worktree has uncommitted lane changes.",
  "Next commands:",
  "  git status --short",
  "  git add <files>",
  '  git commit -m "<message>"',
  '  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."',
  "Or report WIP with:",
  '  codeterm agent report --status partial "..."',
  "Dirty lane files:",
  "  cli/src/cmd/install.rs",
  "  src-tauri/src/service/pty_io/submit_transaction.rs",
  "  .codeterm-build-check.log",
  "  .codeterm-rem-tmp/a2_counts.env",
  "  .codeterm-rem-tmp/a2_verify.txt",
  "  .codeterm-rem-tmp/calibration_count.txt",
  "  .codeterm-rem-tmp/calibration_log.txt",
  "  .codeterm-rem-tmp/create_project_dreams.txt",
  "",
  "Original readiness report that was refused locally:",
  "STATUS: in_review; CHANGES: pl_ghk358 fully structured — GOAL (vector+vision+2 constraints+2 postulates), spec sp_gh3vma 5 sections. TESTS RUN: bash scripts/test-capped.sh smoke-verified against real cargo. GATES REQUESTED: codeterm plan approve pl_ghk358. FOLLOWUP: dispatch task_6p61ll first.",
].join("\n");

/** Distinctive body lines that must survive parsing and reach the DOM. */
export const CURIE_PARTIAL_REPORT_BODY_MARKERS = [
  "REPORT NOT SENT",
  "readiness report refused: this git worktree has uncommitted lane changes.",
  "  src-tauri/src/service/pty_io/submit_transaction.rs",
  "Original readiness report that was refused locally:",
];
