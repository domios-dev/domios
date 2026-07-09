---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8360c00dfd89436f99d1ebd5a831adef
handle: t12-loop-repair
title: 'Loop mechanics: wire iteration recording + visibility'
status: done
---

Loop mechanics: verify-then-repair (spec section loop-mechanics; wave w12-exec-mechanics part A). Discovery is step 1 of THIS task, not a separate wave.

Steps:
1. Trace: who calls loop_engine::record_iteration/advance in the live daemon? Oracle: loop_d86006892c13428f905df19eb7b9830e on plan pl_dd93d764e1b94293b5e943af71dfdb5b shows count 0 after two real replanning iterations — confirm nothing records iterations from the workflow.
2. Wire recording: `codeterm plan loop record <loop-ref> --note "..."` increments count, evaluates budget/stop_when, flips status (Running -> Exhausted/Stopped) with a stride on the plan. Optional automatic trigger: a planner replan stride referencing the loop records an iteration — implement only if clean, otherwise document manual-record discipline.
3. Visibility: heartbeat/reconciliation envelope lists running loops (target, count/budget); doctor WARNS on stalled running loops (no iteration for N days, N configurable constant) and on exhausted-but-running.
4. Stages decision: LoopSpec.stages has no CLI surface — either add one or delete the field; document the decision in the ORCHESTRATION doc loop section (full lifecycle + commands).
5. Tests: record/exhaust/stop paths, envelope rendering, doctor warnings.
6. Backfill oracle: record the two real iterations of loop_d8600689 on this plan; verify count 2/3 shows in plan state.

Commands: narrowed cargo test; ./build.sh check; ./build.sh cli; live oracle check on this plan.
Report: trace findings, wiring implemented, stages decision, oracle before/after (count 0 -> 2).
