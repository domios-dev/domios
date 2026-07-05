---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3165f16070644049bcedb29a924693d8
handle: release-gate-wiring-state-invariants-mirror-concurrency-as-standing-gate
title: 'Release-gate wiring: state invariants + mirror concurrency as standing gates'
status: pending
---

PREVENTION LAYER from the state-drift retro (analysis on task_1d0572d0). Once the P1s land (invariants task_7140a28c, mirror fix task_7089039c, observability task_03bf8da6, watchdog task_1d0572d0), wire them into the gates so the class cannot re-ship: (1) plan doctor gains --kind integrity: runs the transition-invariant check over a LIVE plan (every in_progress has a live-or-flagged pane, every in_review has a report stride, every blocked has a label/gate) — exits non-zero on violations; (2) the develop-PR / release checklist (task_b3789e40 and the release-prep runbook) adds two required lines: 'plan doctor --kind integrity green on the active plan' and 'mirror concurrency suite green' (the permanent suite from task_7089039c); (3) milestone regression gates (shipped, task_96e4ae09) add the integrity check to their per-group-close suite; (4) PROCESS/PROMPT line (ride an existing prompt lane or a one-line follow-up, do NOT open a new prompt file): orchestrator gating checklist references the In-QA queue as the DEFAULT for multi-lane gating — the machinery shipped in task_49f03e58 sat unused during the 3h incident; shipped-but-unadopted is this plan's recurring failure mode (plan skills, In-QA — two occurrences), so the reflection loop's skills-adoption counter should also count In-QA usage.
