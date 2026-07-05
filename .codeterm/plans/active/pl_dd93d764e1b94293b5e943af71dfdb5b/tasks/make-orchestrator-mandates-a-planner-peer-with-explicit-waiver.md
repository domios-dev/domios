---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_aa01c0db6fcf4a679af018805d13c120
handle: make-orchestrator-mandates-a-planner-peer-with-explicit-waiver
title: Make-orchestrator mandates a Planner peer (with explicit waiver)
status: done
---

## What

When ANY agent becomes an Orchestrator through the CodeTerm command path (pane menu Make Orchestrator, `codeterm orchestrator` promotion, plan claim), the pairing becomes MANDATORY: the command's response must tell the agent its next obligatory step — spawn its Planner peer — with the exact spawn command, and the system nudges until the pair exists.

## Design

1. **Command response.** The promotion/claim command output (and the onboarding message the daemon sends the pane) includes a mandatory-next-step block: "You are now an Orchestrator. An Orchestrator works with a Planner peer — spawn one now: codeterm agent spawn <provider> --role Planner --plan <id> ... The planner owns GOAL/Spec/Plan structure; you own dispatch, gates, integration. Do not author plan structure yourself." (shared constant; the spawn command pre-filled with real ids).
2. **Enforcement.** While an active plan has an orchestrator ref but no planner_ref: (a) plan doctor emits a warning (planning-mode) escalating to error at approval time; (b) the orchestrator reminder nudge prepends a compact "no planner paired — spawn one or explicitly waive with codeterm plan update --planner none --note <why>" line (waivable: solo mode is allowed but must be an explicit recorded decision, not a default drift).
3. **Planner spawn wiring.** `agent spawn --role Planner --plan <id>` binds planner_ref automatically and delivers the planner profile prompt naming the orchestrator peer.
4. Depends on planner_ref model field (coordinate with the paired-creation task — one field, one migration).

## Files

cli orchestrator/agent spawn cmd + args, daemon promotion path + onboarding message, plan doctor, reminder nudges, prompt shared constants, ORCHESTRATION doc section (peer contract).
