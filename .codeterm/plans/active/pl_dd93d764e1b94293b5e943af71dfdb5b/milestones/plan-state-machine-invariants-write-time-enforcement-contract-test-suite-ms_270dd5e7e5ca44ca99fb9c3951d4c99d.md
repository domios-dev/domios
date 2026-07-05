---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_270dd5e7e5ca44ca99fb9c3951d4c99d
handle: plan-state-machine-invariants-write-time-enforcement-contract-test-suite-ms_270dd5e7e5ca44ca99fb9c3951d4c99d
title: status_update
---

IN_REVIEW: STATUS: in_review (PARTIAL — the #209 slice of this task, surgical)
BRANCH: lane/7140a28c-209-pane-overwrite @ 6e08243f (base be485acc)
WORKTREE: /Users/rollacode/Developer/codeterm-lanes/209fix-lane-overwrite
CHANGES: the #209 stale-executor root cause FOUND AND FIXED. Root cause (task_ops.rs:233, cited in the task spec as predicted): update_task_lane applied fill-if-empty semantics to assigned_pane_id even for EXPLICIT pane updates — lane.pane_id changed but assigned_pane_id (the field Board/watchdog/reconcile read) kept the old pane whenever one existed. Exactly the '#209 stayed on old pane' symptom. Fix: pane_explicit flag (outer Some on TaskLaneUpdate.pane_id) -> assigned_pane_id mirrors the normalized lane pane (overwrite AND explicit clear); implicit updates keep fill-if-empty. Non-terminal only, terminal path unchanged. Two regression tests appended to fill_lane_tests: explicit_pane_update_overwrites_assigned_pane (#209 repro) + implicit_lane_update_still_fills_only_empty (fill semantics preserved).
TESTS RUN: none (code-only window).
GATES REQUESTED: cargo test fill_lane_tests + service::plan::mutations; the full transition-invariant scope of this task (in_progress-requires-pane, blocked-requires-label, RPC lane-before-status reorder) remains OPEN — my analysis is recorded in the earlier perf-lane report: validate_task_status_transition exists at mutations/mod.rs:185 (already denies InReview->Done and bare Parked), the RPC calls update_task BEFORE update_task_lane (plan_task_rpc.rs ~391 vs 419) so same-command --status --pane needs the lane applied first. Next worker starts there.
FOLLOWUP: recommend merging this slice independently — it fixes a live state-truth bug the Board depends on; the invariants remainder is a clean follow-on.