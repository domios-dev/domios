---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_269300a4b53b4c10aaa5ae5692227b42
handle: prompts-cross-machine-orchestration-etiquette-ga-redirects-orchestrators-ms_269300a4b53b4c10aaa5ae5692227b42
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/dc526c35-prompt-inserts @ 668c76fc (base be485acc)
WORKTREE: /Users/rollacode/Developer/codeterm-lanes/dc526c35-prompt-inserts
CHANGES (5 files, +52/-1 — all five pinned texts applied VERBATIM from the task strides): TEXT A general_agent.md 'Machine scope, not project scope' bullet after the Orchestrator-mode bullet; TEXT B orchestrator_suffix.md cross-machine paragraph before the worker-cleanup paragraph; TEXT C blocked-triage bullet appended to Heartbeat rules; TEXT E ask-planner sentence appended to Planning-ownership + the symmetric 'decisions and rationale, not options' line in planner_suffix.md; TEXT D placed in docs/ORCHESTRATION.md as its own section before Worker Lanes (rationale: orchestration_toolset.md was deleted by the dead-prompts lane, mechanics belong in tier-3, suffix budget preserved) — includes the watcher self-close tail per the observability amendment; ORCHESTRATION revision 25->26. Six wiring tests added in prompts.rs locking all key phrases (Machine scope / Cross-machine work / a claim not a parking spot / ask your Planner before guessing / decisions and rationale).
TESTS RUN: none (code-only window).
GATES REQUESTED: cargo test prompts (six new + existing suite; peer suite 50/50 regression); manual read-through of both suffixes for budget/flow; the evals/cli-agent GA-redirect scenario from the task body remains OPEN — it needs a live drill and stays this task's last criterion, run it at your gate or split to the QA runner.
FOLLOWUP: the task's eval criterion (GA redirect baseline vs after) is the only unfinished piece — everything textual is done.
