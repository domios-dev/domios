---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: section
id: sec_6ae4e33432b34b88aa5594e4afd8f550
handle: w12-exec-mechanics
title: Wave 12 — Loop repair + competitive execution
---

Wave 12 (execution-mechanics deepening; rust lanes, independent of round-2 UX waves 9-11):

A. Loop mechanics repair (spec: loop-mechanics): verify-then-fix in ONE task — trace who calls loop_engine::record_iteration in the live workflow (suspicion: nobody), then wire iteration recording (CLI plan loop record + optional automatic trigger on planner replan strides), surface running loops in the heartbeat envelope, doctor warnings for stalled/exhausted loops, stages field: real CLI or removal (document decision). Areas: src-tauri/src/plan/loop_engine.rs, service/plan/loop_mutations.rs, heartbeat.rs, doctor plumbing, cli plan loop args, ORCHESTRATION doc.

B. Competitive execution (spec: competitive-execution): extend task_attempts.rs from sequential-respawn to concurrent-competition — competition meta on task (mode + competitor count), N concurrent attempts each with mandatory isolated worktree lane + per-attempt model/executor_role overrides, per-attempt zero-flag report resolution (pane -> attempt), judging verdict command (winner/rejected + reason), loser lanes flow into cleanup debt, grouped competition rendering in heartbeat/nudges + plan state. Areas: service/plan/task_attempts.rs + mod.rs task model, lane_metadata.rs, heartbeat.rs, cli plan task args (compete/judge), agent report resolution (composes with p1c), docs (ORCHESTRATION + agents.md judge guidance).

Sequencing: A before B in the same lane is acceptable (B touches heartbeat/doctor surfaces A also touches); B depends conceptually on cleanup-hygiene (t0e) landing for loser-lane debt, but only for the debt hookup — core competition model can proceed.
Verification: narrowed cargo tests per area; ./build.sh check; live dogfood: create a scratch competitive task with 2 competitors on dev, both report, judge, verify winner integration + loser debt + history.
