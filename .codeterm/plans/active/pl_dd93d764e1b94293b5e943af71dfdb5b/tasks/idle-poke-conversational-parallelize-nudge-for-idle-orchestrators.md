---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_663d4887d62d4347af9896db199d3968
handle: idle-poke-conversational-parallelize-nudge-for-idle-orchestrators
title: 'Idle poke: conversational parallelize nudge for idle orchestrators'
status: done
---

## What

A lightweight "idle poke" beside the official heartbeat: when an orchestrator pane sits idle while its plan has dispatchable work, send it a short conversational question in the user's voice — "Is there anything to parallelize alongside the current lanes? (5 pending, 1 live)" — instead of another block of boilerplate rules. The recognizable heartbeat format gets pattern-blindness; a human-sounding question breaks it.

## Trigger (state-gated, never blind-timer)

Fire only when ALL hold:
1. Pane status == Idle continuously >= `initial_idle_secs` (default ~600s). Status source: existing PaneStatusTracker — no new polling.
2. The pane owns an active execution-mode plan whose frontier is non-empty: >=1 pending task that is unblocked (deps satisfied, not parked/cancelled).
3. Live worker lanes below the plan's effective parallelism (if QA pipeline config exists, use it; else a simple cap config).
4. Pane is NOT Waiting (Waiting = asked the user something; poking then is noise).

## Anti-spam (the user's explicit worry)

- Exponential backoff per pane: first poke after initial_idle_secs, then x3, then x6; reset on any dispatch/status change to Working.
- Shared budget with plan reminder nudges: if a reminder fired in the last backoff window, skip the poke (one channel speaks at a time).
- Hard cap `max_per_day` per pane (default ~6).
- Novelty: rotate 3-4 phrasings (shared constants, dedup discipline) and ALWAYS include live numbers (N pending, M live lanes) so consecutive pokes differ and carry real signal.
- Config block `plan.idle_poke.{enabled, initial_idle_secs, backoff_factor, max_per_day}`; enabled=true default, killable per install.

## Delivery & framing

Ride the existing reminder channel (`-=-codeterm:reminder ...-=-` marker — honest in transcript) but the BODY is the plain conversational question, no contract/exits boilerplate. Template examples (shared constants): "Is there anything to parallelize with the current lanes? (N pending, M live)"; "You have N pending unblocked tasks and only M lane(s) running — worth dispatching more?"; "Frontier check: anything dispatchable while X is in flight?". Optionally append the top-3 pending task titles.

## Placement

Daemon-side module next to plan_reminder_nudge (service/hierarchy) consuming PaneStatusTracker idle transitions + plan store frontier; shares the nudge delivery path. All wording as shared constants; tests reference constants (no re-typed prose).

## Out of scope

Poking Workers (their charter nudges cover them), cross-pane escalation to the user, LLM-generated phrasing.
