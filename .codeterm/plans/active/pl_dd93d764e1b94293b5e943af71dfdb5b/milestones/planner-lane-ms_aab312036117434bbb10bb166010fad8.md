---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_aab312036117434bbb10bb166010fad8
handle: planner-lane-ms_aab312036117434bbb10bb166010fad8
title: status_update
---

PARTIAL: CLI id ergonomics planned (user screenshot feedback).
SPEC: new section id-ergonomics: (1) parent-free addressing — task ids are store-unique, so task-scoped commands take the task ref ALONE, plan id optional disambiguator (same for sections); (2) short salted ids for NEW entities — 6-8 char base36 slug with type prefix (task_x7k2mq), collision-checked at mint, salt regenerated on collision; old long ids resolve forever, no migration/shims; (3) unified ref resolution everywhere: full id | unique prefix | handle, ambiguity errors list candidates, never guess; (4) human outputs print short id/handle first, --json keeps full ids.
PLAN/TASK: wave w0d-id-ergonomics + task t0d-id-ergonomics (opus, p2, wave0 group): shared daemon-side resolver + minting change + CLI wiring + output pass + docs; coordinates with wave-p1 lanes on shared service/plan files.
DEPENDENCY: not gating P0/P1 or UX lanes; pure dogfood ergonomics, dispatch parallel in wave0.
STATE: 18 tasks, coverage 100%. NEXT: unchanged — P0 lane active, p1-sidecar-routing wants its rust worker; t0d can go to any free rust worker after p1 lanes are staffed.
