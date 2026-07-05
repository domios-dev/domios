---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_a2c46c03a8674d1397df16a97038a9a5
handle: worker-lanes-code-only-report-refusal-hardening-qa-queue-adoption-build-ms_a2c46c03a8674d1397df16a97038a9a5
title: decision
---

TRANSITION-TIME REINFORCEMENT (round-72, user requirement — the rule must arrive AT THE MOMENT, not only at spawn): the in_review transition itself teaches the QA invariant. ADD TO SCOPE: (a) the SUCCESS response of 'agent report --status in_review' (and 'plan task update --status in_review') appends one hint line: 'Heavy gates run in this machine's single QA lane — your GATES REQUESTED lines are queued via plan qa; do not run builds/suites yourself.' — same hint mechanism the CLI already uses, one line, product-general; (b) if the report contains NO 'GATES REQUESTED' section, the hint becomes a prompt: 'No gates requested — if this change needs build/clippy/suite verification, list them under GATES REQUESTED or the orchestrator will decide.'; (c) both hints respect the output contract (task_dd7206eb: hints never displace the machine-readable last line). DELIVERY MOMENTS now total three, all automatic, zero orchestrator memory required: SPAWN (worker_suffix charter line — every worker born knowing), TRANSITION (this hint at in_review — the rule arrives exactly when testing temptation peaks), VIOLATION (registry-keyed build detection nudge — the backstop when both were ignored). Acceptance addition: transition drill shows the hint on report; no-gates-requested drill shows the prompt variant.
