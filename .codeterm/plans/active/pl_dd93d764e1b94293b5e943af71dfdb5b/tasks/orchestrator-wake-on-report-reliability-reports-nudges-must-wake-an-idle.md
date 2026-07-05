---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fff79b091f02449cbc0b96259952c793
handle: orchestrator-wake-on-report-reliability-reports-nudges-must-wake-an-idle
title: 'Orchestrator wake-on-report reliability: reports/nudges must wake an idle orchestrator'
status: done
---

INCIDENT (2026-07-04, this plan): worker 2413e804 filed a full in_review report on task_cc1ed23b; orchestrator Puccini (codex pane 4d479b6c) sat idle for ~35 minutes and never processed it — only a manual 'codeterm send' woke him. The [Report from ...] auto-injection contract says reports arrive in the orchestrator terminal; either the injection did not fire, or it fired while the provider was idle and did not trigger a generation (injected text sitting in the composer/scrollback wakes nothing). Investigate the full wake chain for an IDLE agent pane: report emission -> pane injection -> provider generation trigger, for codex AND claude providers (capability-keyed, not provider-id-keyed). Reproduce: spawn orchestrator, let it go idle, have a child file 'codeterm agent report', measure time-to-first-orchestrator-action. Same audit for reconciliation nudges and decision-gate notifications. Fix likely needs an explicit submit/enter after injection when the pane is idle, or a daemon-side wake nudge when injected content is not consumed within N seconds. NOTE: an interim behavioral mitigation (orchestrator polls plan state before going idle) is already prompted — this task fixes the mechanism so the mitigation stops being load-bearing.
