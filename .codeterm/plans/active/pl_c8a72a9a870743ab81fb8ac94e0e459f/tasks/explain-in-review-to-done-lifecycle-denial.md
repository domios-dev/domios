---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_fc49ff86fc134c31ae70352acf8ead1d
handle: explain-in-review-to-done-lifecycle-denial
title: Explain in-review to done lifecycle denial
status: done
---

Improve CLI explanation for task status transition denial.

Context:
A user ran:
  codeterm plan record <plan> --task <task> --status done --step "testing:..."
and got:
  RPC error -32603: task status transition InReview -> Done is not allowed

The rule is correct: workers report into in_review; the Orchestrator accepts via plan task close. The CLI explanation is bad.

Scope:
- Render PlanError::TransitionDenied in cli/src/cmd/plan_errors.rs.
- Special-case in_review -> done with a clear explanation:
  - in_review means worker reported and awaits orchestrator acceptance.
  - use codeterm plan task close <plan> <task> --result "..." to accept.
  - if review found more work, bounce with task update --status in_progress --status-label "needs changes".
- Add focused CLI renderer test.
- Keep backend lifecycle rule unchanged.

Acceptance:
- The same RPC error renders actionable guidance instead of raw transition-denied text.
- Generic transition_denied still has reasonable fallback.

Tests:
- cargo test --manifest-path cli/Cargo.toml cmd::plan::tests::renders_in_review_done_transition_fix -- --nocapture
- cargo test --manifest-path cli/Cargo.toml cmd::plan::tests -- --nocapture
