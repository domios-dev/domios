---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_96e4ae090c0d40119a04aeb81303207f
handle: milestone-regression-gates-full-suite-once-per-group-close
title: 'Milestone regression gates: full suite once per group close'
status: done
---

User directive (round-36): per-task testing stays ISOLATED (focused filters — already the rule); FULL regression runs once per MILESTONE, not per task. The planner already supports task groups ('codeterm plan groups' = phases) but groups trigger nothing. WIRE THEM: (1) when the LAST task of a group closes, next-actions/heartbeat emit a regression-gate suggestion ('group <name> complete — run the full gate once: cargo suite + ts-unit + playwright golden, serialized in the warm checkout / remote-test farm'); (2) the QA pipeline gets a group-scope claim ('plan qa next --group <name>') so the regression run is recorded as ONE working point on the group, not scattered; (3) doctor warns if a group closed without its regression record (structural check: group complete + no regression milestone); (4) docs: ORCHESTRATION gains a 'Milestone gates' paragraph — per-task = focused only, per-group = one full regression, per-release = farm matrix. Executable test plan: unit — group-completion detection matrix (last-task-close fires once, reopening a task re-arms); CLI round-trip on dev with a seeded 2-group plan (close group -> suggestion appears, record regression -> doctor silent); prompt constant wiring test.
