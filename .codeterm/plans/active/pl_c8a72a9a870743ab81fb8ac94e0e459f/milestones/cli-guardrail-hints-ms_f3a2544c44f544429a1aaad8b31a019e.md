---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_f3a2544c44f544429a1aaad8b31a019e
handle: cli-guardrail-hints-ms_f3a2544c44f544429a1aaad8b31a019e
title: status_update
---

Rebased feat/cli-hints onto origin/feat/canvas and force-pushed b4d34037. Resolved batch.rs/batch/tests.rs by keeping the feat/canvas apply_batch atomic staging model and layering cli-hints on top: _hints result field, task creation hints, @latest task selector, set_task_sections, and set_task_status all mutate only the staged working plan. Kept BE-fix atomicity tests and added/kept section-link tests, including rollback coverage for set_task_sections. Did not run build/cargo/tests per orchestrator instruction.
