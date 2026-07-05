---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a9fed2880cb44a20ad411af074f9abfa
handle: gh-207-179-cron-hidden-agent-reliability-auth-outage-recovery-skills-loa
title: 'GH #207 + #179: Cron/hidden agent reliability — auth-outage recovery + skills loading'
status: done
---

Two hidden-run issues, one lane. STEP 1: gh issue view 207 && gh issue view 179. (A) #207: after a provider auth outage, cron hidden agent runs keep failing and the task prompt is not reliably submitted. NOTE: the submit half may already be fixed by lane/wake-on-report-reliability (Messenger::deliver wake confirmer, task_fff79b09) — verify against that lane's build first; the remaining scope is auth-outage recovery (retry/backoff when the provider session is invalid, surfacing the failure instead of silent no-op). (B) #179: cron/spawned agents cannot load ~/.codeterm/skills/ via the Skill tool — silent failure, and a home-wide find can OOM; fix the skills path resolution for hidden/cron runs and bound any directory scan. If parts are already fixed on branch, close with evidence.
