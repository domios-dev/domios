---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c15b29dfd7d440a6b37295d4023bf298
handle: worker-charter-loop-competitive-context-rules
title: 'Worker charter: loop + competitive context rules'
status: done
---

## Findings (investigation done by Planner)

- **Orchestrator side:** the heartbeat block already renders running loops — `render_running_loops` in `src-tauri/src/service/plan/heartbeat.rs:60` emits "Running loops: <id>: <target> (count/budget)" into every `<codeterm-orchestrator-heartbeat>`. Loops are visible to the Orchestrator.
- **Worker side: NOTHING.** `render_worker_charter` (`src-tauri/src/service/plan/worker_charter.rs`) is the generic three rules + optional role_guidance. A worker executing a loop-targeted task never learns the iteration count, budget, or stop_when; a worker running one of several competitive attempts (`PlanTask.execution_attempts`) gets zero rules about attempt isolation. `execution_attempts` never surface in any nudge or heartbeat.
- role_guidance auto-injection is already correct: `prepend_worker_charter_for_pane` prepends "Role guidance: ..." + charter to every nudge/delayed send for the assigned pane (`delayed_send.rs:422`, `plan_reminder_nudge.rs:172`). No work needed there.

## What to build

**1. Loop context in the worker charter.** When the pane's open task is targeted by a Running loop, append to the charter: "Loop <id>: iteration <count>/<budget>. Stop when: <stop_when|budget exhausted>. Record iteration evidence with `codeterm plan record --step iteration:<what changed>`; repeating a failed approach unchanged counts as stagnation." Resolve via plan.loops target matching the task id (same resolution the loop-iteration-badge task task_79f0de5336584b4db512a4136e19d397 uses — share the lookup if practical).

**2. Competitive-attempt rules in the worker charter.** When the task has >1 live execution attempts, append: "You are attempt <n> of <N> running concurrently. Work ONLY in your own worktree/lane; do not inspect or coordinate with competing attempts; do not merge or claim the task done — report honestly and the Orchestrator judges attempts."

**3. Competitive attempts in the orchestrator heartbeat.** Next to "Running loops", render "Competitive attempts: <task-name> — <N> live" so the judging duty stays in the frontier.

## Dedup discipline (hard requirement, per user)

Prompt wording lives ONCE: define the loop/competitive snippets as shared constants/templates in one module and include them from charter render (and docs if quoted). NO prompt-text contains-asserts duplicating prose in tests — test the render functions behaviorally (given a loop-targeted task, output includes the shared constant; reference the constant in the assert, never a re-typed literal).
