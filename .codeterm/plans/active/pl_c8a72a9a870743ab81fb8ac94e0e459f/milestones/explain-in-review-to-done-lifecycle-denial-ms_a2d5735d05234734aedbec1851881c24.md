---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_a2d5735d05234734aedbec1851881c24
handle: explain-in-review-to-done-lifecycle-denial-ms_a2d5735d05234734aedbec1851881c24
title: status_update
---

Accepted. CLI now renders transition_denied with a specific in_review -> done explanation: workers cannot close their own reviewed work; orchestrator accepts via codeterm plan task close, or bounces via task update --status in_progress --status-label needs changes. Gates rerun: cli cmd::plan::tests 14/14, git diff --check.
