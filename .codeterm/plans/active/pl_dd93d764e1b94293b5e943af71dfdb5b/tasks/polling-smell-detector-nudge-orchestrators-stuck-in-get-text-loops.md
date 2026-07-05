---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_25dbe180263b476eb98f4f5c8ff0cc48
handle: polling-smell-detector-nudge-orchestrators-stuck-in-get-text-loops
title: 'Polling-smell detector: nudge orchestrators stuck in get-text loops'
status: pending
---

From the Puccini polling incident (see analysis stride on task_d4105813): prompts alone lose to friction under urgency — add a mechanical backstop. DETECT: an orchestrator pane (owns an active plan) issuing >=3 'pane get-text'/'pane status' CLI calls against the SAME target pane within 10 minutes. All CLI calls dispatch through the daemon, so the counter lives server-side (leads: socket server dispatch + the existing nudge plumbing in src-tauri/src/service/hierarchy/plan_reminder_nudge.rs; config under [orchestration] like reflect_every_gates). ACT: ONE nudge into the orchestrator pane (rate-limited per target per hour) quoting the gate pattern: 'You are polling <pane>. Convert to a self-reporting gate: <cmd>; codeterm send --pane ... GATE:<task> rc=0 — record it and work the frontier.' Never nudge non-orchestrator panes (a human user get-texting is fine — key on the pane's plan-owner role, not on the CLI caller identity alone). Include unit tests for the counter window + rate limit; one live drill on dev.
