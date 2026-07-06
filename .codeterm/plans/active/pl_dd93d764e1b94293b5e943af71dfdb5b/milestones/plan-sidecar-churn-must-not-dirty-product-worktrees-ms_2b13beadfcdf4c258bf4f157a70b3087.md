---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_2b13beadfcdf4c258bf4f157a70b3087
handle: plan-sidecar-churn-must-not-dirty-product-worktrees-ms_2b13beadfcdf4c258bf4f157a70b3087
title: decision
---

PHASE 2 GATE DECIDED BY ANDREY (round-107, live evidence: 135-file normalization walls on every merge, runtime churn rewriting plan structure under his hands — verbatim: 'я их мерджу, оно постоянно что-то меняется, вся структура, все файлы'). DECISION = OPTION B: the daemon's plan runtime writes MOVE OUT of the git worktree into ~/.codeterm state (per-project plan store keyed by project path); the repo receives plan state ONLY via explicit 'codeterm plan export' (reviewed snapshot commits on demand — history-in-git preserved as a deliberate act, never as churn). Scope: all continuous writers (strides, status, sidecar markdown, INDEX, plan.json) relocate; the watcher watches the state dir; existing in-repo plan history stays as-is (frozen archive — do not delete); a one-time migration imports current .codeterm/plans into the state store; after migration, .codeterm/plans gets a README pointing at export. Priority: FIRST POST-RELEASE LANE — it is actively hostile to the user's daily git workflow. Sequencing note: not before the release (storage rewiring during endgame is forbidden by the same logic as stable-ids).
