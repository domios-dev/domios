---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_49f03e589ae24119bd4affe32ded3e81
handle: qa-pipeline-in-qa-stage-serialized-test-queue
title: 'QA pipeline: In-QA stage + serialized test queue'
status: done
---

## What

Add an **In QA** sub-stage to the review phase AND make it the backbone of a **serialized QA pipeline**: implementation parallelizes freely across worktrees, but heavy verification (full builds, broad test suites) runs through a single-slot QA lane with a queue. This turns "don't storm the CPU with N parallel cargo builds" from a prompt-level vibe into plan mechanics any orchestrator follows automatically.

## Why (user pain, observed)

Orchestrators refuse to parallelize implementation because every parallel worktree that starts testing also starts building — N concurrent builds thrash even strong machines. The correct shape: workers run only cheap lane-local checks; full gates are serialized in ONE warm integration checkout by a dedicated QA executor; failures bounce back to the implementing worker. Today this exists only as prompt wording an orchestrator may or may not internalize.

## Model

1. `review_stage: Option<ReviewStage>` on PlanTask — `ReviewStage { Review, Qa }` — only meaningful while status == InReview; cleared on any transition out.
2. QA queue semantics on top: `qa_state: queued | running` (part of the stage payload). The plan store enforces **at most ONE task with qa_state=running per plan** (config `plan.qa.max_concurrent`, default 1; the enforcement is the CPU gate — mechanical, not advisory). FIFO by enqueue time; position derivable.
3. Entering QA requires evidence (procedural sealing): the transition writes a milestone (kind `qa`) atomically — enqueue note, claim note, verdict note all land in the task activity trail with author pane + timestamp.

## Flow

- **Worker finishes** → reports in_review as today. Orchestrator (or the report path directly, config flag) enqueues: `codeterm plan task update <ref> --review-stage qa --note "ready for gates: <what to verify>"` → qa_state=queued.
- **QA executor** (a Worker with executor_role `qa`/`tester`, running in ONE designated warm integration checkout — NOT a fresh worktree per task) pulls work: `codeterm plan qa next --claim` (zero-flag pane identity) → head of queue becomes running, claim milestone written. The command prints the task's test_plan + lane branch to merge/checkout.
- **Verdict:** `codeterm plan qa pass <ref> --note "..."` → stage returns to Review, orchestrator gates/closes as usual. `codeterm plan qa fail <ref> --note "<failure evidence>"` → task bounces to in_progress, failure milestone written, and the implementing worker's pane gets an automatic nudge with the failure note (reuse the reminder/nudge plumbing).
- **Queue visibility:** `codeterm plan qa queue` lists queued/running with ages; orchestrator heartbeat renders "QA queue: N queued, task X running (age)"; next-actions suggest claiming when a qa-role lane exists and queue is non-empty, or spawning a tester when queue depth > threshold.

## Prompts/rules (dedup discipline — shared constants, no re-typed prose in tests)

- Worker charter constant: "Run only narrow lane-local checks in your worktree; full builds/suites happen in the QA lane. Report in_review when done — do not run broad gates yourself unless the task explicitly says so."
- Orchestrator heartbeat rule constant: "Parallelize implementation lanes freely; heavy verification is serialized through the QA queue — never gate a worktree with a full build."
- ORCHESTRATION doc: QA pipeline lifecycle section (enqueue → claim → verdict → bounce), when to spawn a dedicated tester, and the single-warm-checkout rationale (cold worktree builds are the CPU storm).

## UI

- In QA chip in board/list/detail (distinct tint from review), plus qa_state (queued position / running) in the tooltip and task detail. In_review filters keep matching QA tasks (sub-stage, not sibling status).

## Out of scope

Automated test execution by the daemon itself, cross-machine QA lanes (mesh tester — follow-up), multi-slot QA (config exists but default stays 1).
