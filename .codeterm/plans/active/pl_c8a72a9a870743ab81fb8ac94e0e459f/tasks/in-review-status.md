---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5cf3fa6da5b044f8a11443cb548534f0
handle: in-review-status
title: 'Add ''in_review'' task status (§38: worker→in_review, orchestrator→done)'
status: done
---

§38 acceptance: workers report in_review, orchestrator decides done. Status enum lacks in_review (pending/in_progress/done/cancelled/blocked) — CLI rejects --status in_review. Add in_review to enum (src-tauri plan model + CLI + canvas render); only orchestrator transitions in_review→done; canvas shows in_review distinctly. Tests for status + transition.
