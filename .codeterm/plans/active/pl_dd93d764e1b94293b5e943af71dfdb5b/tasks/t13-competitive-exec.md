---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c4d48f0adfb841c0849ac095e4690852
handle: t13-competitive-exec
title: 'Competitive execution: concurrent attempts + judging'
status: done
---

Competitive multi-agent execution (spec section competitive-execution; wave w12-exec-mechanics part B). Sequence after t12-loop-repair in the same lane (shared heartbeat/doctor surfaces). User-opt-in feature — never a default.

Steps:
0. **Declarative competitive marking at planning time (round-6 addition).** The task itself can carry the intent BEFORE any dispatch: `plan task add/update --competitive N [--competitive-models a,b] [--competitive-hint "judge on: ..."]` writes competition meta (desired_attempts, optional model split, judge hint) onto the task. This is the Planner/user surface — marking a task competitive is a planning decision, not a dispatch-time improvisation.
1. Task model: competition meta (mode + competitor count) on the task; task_attempts.rs extended from sequential-respawn to N CONCURRENT open attempts, each with full lane metadata (pane, branch lane/<handle>-c<N>, mandatory isolated worktree) and optional per-attempt model/executor_role overrides. Meta written by step 0 is the same struct `compete` consumes.
2. CLI: `codeterm plan task compete <task-ref> --competitors N [--models a,b] [--roles x,y]` creates the attempt skeletons + prints per-competitor spawn commands (or spawns via existing agent spawn plumbing if clean); with a step-0 marked task, `compete` with no flags uses the recorded meta. `codeterm plan task attempt judge <task-ref> --attempt <n> --verdict winner|rejected --reason "..."` records judgment.
3. Status semantics: task in_progress while any attempt open; all-reported -> judging state visible in plan state/task show; winner integrates via the normal orchestrator gate; loser lanes become cleanup debt (compose with t0e cleanup-hygiene — hook only, degrade gracefully if t0e unlanded); all attempts persist in executor history.
4. Reports: per-attempt zero-flag resolution — reporting pane maps to ITS attempt (extends p1c pane-identity resolution).
5. Heartbeat/nudges: a competition renders as ONE grouped lane (task handle + N competitors + per-attempt state). **Pre-dispatch guidance (round-6 addition): when the dispatch frontier reaches a competitive-marked task, the next-actions Dispatch entry and the reminder nudge explicitly say "task X is marked competitive xN — launch via `codeterm plan task compete X`", including model split and judge hint when recorded. `task show` renders the marking prominently. Doctor (quality) warns when a competitive-marked task was dispatched as a single attempt.**
6. Docs: ORCHESTRATION (competition lifecycle, when to use: user request only; marking flow Planner -> dispatch nudge -> compete) + agents.md judge guidance. Prompt wording for the nudge lives ONCE as a shared constant (no re-typed prose in tests).
7. Tests: concurrent attempt lifecycle, judge verdicts, report->attempt resolution, grouped rendering, **marked-task dispatch nudge + compete-uses-meta + doctor single-dispatch warning**; live dogfood: scratch competitive task with 2 competitors on dev — both report, judge, winner integrated, loser lane in cleanup debt.

Commands: narrowed cargo tests; ./build.sh check; ./build.sh cli; live 2-competitor smoke on dev 7686.
Report: model/CLI surface, judge flow, smoke transcript, history/debt evidence, dispatch-nudge rendering evidence.
