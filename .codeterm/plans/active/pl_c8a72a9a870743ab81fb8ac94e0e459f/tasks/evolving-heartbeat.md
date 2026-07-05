---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_a1d84645ccf94c169fb128d7bc75f9f3
handle: evolving-heartbeat
title: Evolving orchestrator heartbeat + learnings log (first-class, append-during-work, re-injected)
status: done
---

# Evolving orchestrator HEARTBEAT + LEARNINGS log (first-class, append-during-work, re-injected)
Andrey: the orchestrator's heartbeat (currently a hand-edited /tmp/heartbeat.md re-armed via `codeterm delayed`) must become a FIRST-CLASS, system-supported mechanism. The orchestrator should APPEND learnings WHILE it works, they PERSIST, and they are RE-INJECTED back into the orchestrator during work — so the learning loop is supported by the system, not manual file editing.

## Mechanism
- A persistent per-orchestrator (or per-plan) HEARTBEAT/LEARNINGS store: the standing state (current focus, active lanes, rules) + an append-only LEARNINGS list (each: short text, ts, optional tag).
- CLI to append a learning mid-work: `codeterm orchestrator learn "<learning>"` (or `plan heartbeat add-learning`). De-dup near-identical learnings.
- The delayed/heartbeat re-injection (and reseed) RESOLVES the current store at send time (state + recent learnings) — so re-injected content is always current, never stale (ties to §34 goal-anchored heartbeat: resolve at send, not at arm).
- Surface in `plan get`/a `codeterm orchestrator heartbeat show`. Optionally render on canvas (orchestrator HUD, ties §36 presence / §34 GOAL HUD).

## acceptance_criteria
- `orchestrator learn "<x>"` persists and survives restart; re-injection (delayed/reseed) includes the latest state + learnings resolved at send time (stale-proof); dedup works.
## test_plan
- unit: append/persist/reload/dedup; resolve-at-send returns current; CLI round-trip + contract.

Ties to: the heartbeat I maintain, §34 GOAL reinjection, §36 presence, orch-doc-reminders, mem rules. This makes "expand the heartbeat with learnings as you go" a supported capability, fed back during work.
