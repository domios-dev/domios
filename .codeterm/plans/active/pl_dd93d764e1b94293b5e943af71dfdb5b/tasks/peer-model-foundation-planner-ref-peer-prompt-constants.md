---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_48f06a32d87b42319d97d2f6bdd43eb8
handle: peer-model-foundation-planner-ref-peer-prompt-constants
title: 'Peer model foundation: planner_ref + peer prompt constants'
status: done
---

## What

The tiny foundation every Wave-21 lane needs, landed ONCE and first. Four pending tasks (paired creation task_7e5f9354, make-orchestrator mandate task_aa01c0db, planning-state task_fe24b9f2, decision gates task_072e7328) plus the Wave-20 companion resolver all require the planner binding and the peer-contract wording. Without this task each lane invents its own field/migration — haphazard digging.

## Scope (deliberately small)

1. **Model:** `planner_ref` on Plan — pane binding symmetrical to the existing orchestrator ref, plus the explicit waiver state (solo mode: `planner: none` + note). Serde, persistence, mirror.
2. **Resolution helper:** one function — planner pane for a plan (planner_ref -> live pane), with orchestrator fallback and none-waiver awareness. This is THE lookup the companion bubble, decision-gate routing, nudges, and doctor all call.
3. **CLI:** `codeterm plan update --planner <pane|none> [--note]` (waiver requires note); `plan get/state` render the pair (orchestrator + planner + waiver).
4. **Peer-contract constants module:** the canonical wording used by all consumers — planner charter line ("planner owns GOAL/Spec/Plan structure"), orchestrator deference line ("structure belongs to your Planner peer"), pairing-mandate block skeleton. Consumers import; tests reference constants (dedup discipline).
5. Doctor: only serialization sanity here — behavior rules stay in the consumer tasks.

## Explicitly NOT here

Pairing UX/flows (task_7e5f9354), promotion mandate + nudges (task_aa01c0db), planning_state machine (task_fe24b9f2), gate routing behavior (task_072e7328). They all DEPEND on this and consume it.
