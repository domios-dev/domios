---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_530d8ab59822494198a4170d25b42d14
handle: report-cli-as-single-planning-update
title: Report CLI as single planning update
status: done
---

Make report UX and semantics crisp. Add or document a top-level codeterm report alias if appropriate. Default --plan/--task from CODETERM_PLAN_ID/CODETERM_TASK_ID. Ensure --status in_review moves task to in_review and records a stride; --status blocked moves task to blocked and notifies owner; --status partial records a stride without closing/changing review state. Decide how worker --status done behaves: either reject with teaching guidance or convert to in_review/claim-done, because Orchestrator owns done via plan task close. Render structured fields branch/sha/changed_files/test_cmd/test_result in the milestone/report and keep owner notification useful.
