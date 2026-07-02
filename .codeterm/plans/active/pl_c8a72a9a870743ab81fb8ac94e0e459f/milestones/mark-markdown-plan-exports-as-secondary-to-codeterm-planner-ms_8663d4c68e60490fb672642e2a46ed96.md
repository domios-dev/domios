---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_8663d4c68e60490fb672642e2a46ed96
handle: mark-markdown-plan-exports-as-secondary-to-codeterm-planner-ms_8663d4c68e60490fb672642e2a46ed96
title: status_update
---

Markdown plan export is now explicitly marked as a snapshot: CodeTerm planner state is authoritative and edits must go through codeterm plan. docs/ORCHESTRATION already states planner source-of-truth. Gates: cargo test cli cmd::plan::tests 13/13 and cmd::manifest 5/5.
