---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_8d37e96e41964fa4b3b9b072ed3ab866
handle: fix-r4-mission-control-memoization-defeated-by-raw-plan-panes-deps-ms_8d37e96e41964fa4b3b9b072ed3ab866
title: status_update
---

Accepted and merged at 1fdc38f9 after resolving with MC-v2. LiveView keeps flow-river/all-plans behavior and keys expensive builders/snapshots/activeWork on missionControlInputRevision via refs. Gates: MissionControlLiveView 1/1 PASS; missionControlInputRevision 3/3 PASS; flowModel 8/8 PASS; PlanRiver 7/7 PASS; MissionControlPlanSurface 3/3 PASS; npx tsc --noEmit PASS; pre-push cargo check/fmt/clippy PASS.
