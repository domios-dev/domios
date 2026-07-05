---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2767c0ed14f845edbf7539b3eb15d675
handle: phase-c-fix-cli-manifest-freshness-test-must-not-hang-ms_2767c0ed14f845edbf7539b3eb15d675
title: decision
---

PLANNER TRIAGE (round-53, user asked why 2 blocked tasks sat unrouted). This task was blocked with label pane-gone — its worker died and drift reconciliation parked it, but nobody took the next action. TRIAGE VERDICT: this is a pure REASSIGN case, not a replanning case — the task body, criteria, and test plan are complete and executable (capped runs of cmd::manifest::tests::cli_manifest_is_fresh + docs_cli_md_is_fresh under 90s; cli-manifest --write no-diff check). Flipped to pending/needs-reassign for redispatch. All 9 Phase-C siblings are done; this is the last one. NOTE: it may already be moot — the hang class was addressed by the runaway sweeper + capped runner work; the reassigned worker should FIRST just run the two tests (verify-first) and close if green.
