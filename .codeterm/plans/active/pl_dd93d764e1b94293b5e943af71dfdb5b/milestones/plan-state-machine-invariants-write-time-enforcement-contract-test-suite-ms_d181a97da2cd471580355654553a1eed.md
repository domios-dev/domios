---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d181a97da2cd471580355654553a1eed
handle: plan-state-machine-invariants-write-time-enforcement-contract-test-suite-ms_d181a97da2cd471580355654553a1eed
title: status_update
---

IN_REVIEW: STATUS: in_review (UPDATED — core invariants now implemented, supersedes the partial report)
BRANCH: lane/7140a28c-209-pane-overwrite @ e1a427f6 (base be485acc, 3 commits)
CHANGES beyond the #209 fix: (1) WRITE-TIME INVARIANTS in validate_task_status_transition (mutations/mod.rs): ->in_progress REQUIRES an executor pane (assigned_pane_id or lane.pane_id non-empty), rejects with the exact fix command; ->blocked REQUIRES a status_label (incoming in the same write or existing on the task) — 'a claim, not a parking spot' mechanically enforced; both behind CODETERM_STATE_INVARIANTS env (default ON, =off for emergencies); existing rules untouched (InReview->Done denial, park-requires-reason, open-subtasks). (2) RPC REORDER (plan_task_rpc.rs): lane update now applies BEFORE update_task, so '--status in_progress --pane X' in ONE command satisfies the pane invariant — the ordering bug my analysis flagged. (3) Two new PlanError variants with actionable Display messages. (4) Threaded incoming status_label through the validator; batch.rs callers updated (UpsertTask passes None — carries no label). TESTS WRITTEN (not run): in_progress_requires_pane_invariant (reject bare -> pass with pane), blocked_requires_label_invariant (reject bare -> pass with label), plus the two #209 lane-semantics tests from the first commit.
GATES REQUESTED: cargo test service::plan (EXPECT FIXTURE FALLOUT: any existing test that flips a task to in_progress without a pane or blocked without a label will now fail — that is the invariant working; triage each as fixture-fix, not code-bug, unless a legitimate flow surfaces); clippy both crates; live drill: update --status in_progress without --pane -> refusal text; same command with --pane -> single-command success (proves the reorder).
REMAINING from the task spec (honest boundary): ->in_review-requires-report-stride NOT implemented (needs stride-store access inside the mutation lock — design decision for the next slice); report-path contract e2e tests are QA-gate work; grandfathering is inherent (only NEW transitions validated). FOLLOWUP: if QA fixture fallout is large, the pragmatic path is a test-fixtures commit on this same lane, not weakening the invariant.
