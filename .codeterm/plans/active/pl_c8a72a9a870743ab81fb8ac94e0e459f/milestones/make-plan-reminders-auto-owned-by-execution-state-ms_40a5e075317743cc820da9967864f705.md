---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_40a5e075317743cc820da9967864f705
handle: make-plan-reminders-auto-owned-by-execution-state-ms_40a5e075317743cc820da9967864f705
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: none
BUILD/TYPECHECK/TESTS: not run; read-only review only. Ran rg/sed/git apply --check inspection. git apply --check /tmp/codeterm-heartbeat-review/heartbeat-nudge.diff applies structurally, but the patch as provided references missing symbols in this worktree.
FINDINGS:
1. BLOCKING: Patch as provided does not compile against the inspected base. It imports/uses crate::plan::PlanMode, reads plan.mode, and tests call PlanStore::set_mode, but src-tauri/src/plan/types.rs has no PlanMode enum and rg found no set_mode implementation. Evidence: diff lines 25, 58, 79, 152, 238, 414, 437; current src-tauri/src/plan/types.rs:7 only defines PlanStatus, then TaskStatus; rg "fn set_mode|enum PlanMode" src-tauri/src cli/src found nothing. If this is meant to be stacked on an unseen PlanMode patch, integrate that prerequisite before gating.
2. HIGH: report_pending_task_binding_for_pane is missing the PlanMode::Execution gate. The owner path correctly adds plan.mode != PlanMode::Execution and worker charter does too, but the worker reminder path now calls this helper and it only checks plan.status == Active plus Pending/InProgress/Blocked. That means a force/draft planning-mode task assigned to a pane can still receive automatic worker report nudges, violating the rule that automatic plan help is execution-mode only. Fix: include plan.mode != PlanMode::Execution in this helper and add a planning-mode assigned-worker negative test.
Notes: No prose keyword heuristic was added. The report fallback/env behavior appears preserved because agent report fallback and PTY env injection still use task_binding_for_pane, not the narrowed reminder helper; InReview tasks should keep CODETERM_PLAN_ID/CODETERM_TASK_ID fallback while no longer receiving report nudges. active_owned_plan_with_open_work_for_pane execution-mode gating is directionally correct and has owner planning-mode coverage in the diff. Worker charter now correctly uses execution mode and report-pending statuses, but lacks a planning-mode negative test.
FOLLOWUP: Apply/stack the missing PlanMode model/API patch or add it here; then add the worker reminder planning-mode negative test and run focused Rust tests for service::plan::reminder_tests, service::plan::worker_charter, and service::plan::heartbeat_tests in the warm orchestrator checkout.
