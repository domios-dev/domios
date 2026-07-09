---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_cd5b22d380754c18af22b08ab6576243
handle: agent-presence
title: Agent PRESENCE — live who's-working-on-what (§36)
status: done
tags:
- presence
---

# §36 — Agent PRESENCE: live "who's working on what" (activity layer)

Andrey: I must ALWAYS be able to see, in the canvas UX, what the orchestrator is doing AND which agent is working on which task RIGHT NOW. Everyone (orchestrator + workers) must continuously surface their current task + current work-point. This is enforced by reminders so nobody silently drifts.

## Model (builds on existing primitives)
- **Claim = take.** An agent working on a task CLAIMS/takes it (from whoever held it) — `task take` / `plan task assign`. Ownership is explicit and transferable.
- **Work-point authorship = activity record.** Each work-point (stride) records WHO wrote it + WHAT they did (author + step). The task's CURRENT owner/worker = author of the LAST work-point. So "who is on this task now" is derived, not separately tracked.
- **Ownership history = work-point author chain.** The sequence of work-point authors over time IS the task's hand-off / transition history — viewable later ("who touched this task, in what order"). Reuse §27 owner_history + strides.
- **Current-activity field.** Each agent publishes a short "current focus" (task id + one-line what) — the orchestrator too. Surfaced live.

## Reminders (the enforcement — everyone keeps it current)
Nudge to update "what I'm working on" via MULTIPLE triggers (not one):
- **Heartbeat / time-based** (the delayed-message cadence already used by the orchestrator) — re-arm carries "state your current task".
- **CLI-use-triggered** — when an agent calls the CLI, a guard-rail hint reminds it to record a work-point / set current focus if stale (extends the cli-hints guard-rail we built).
- **Message-count / cadence-based** — after N turns/messages without a work-point, prompt to report.

## Canvas UX
- Live per-agent activity ON the task: the task a given agent currently owns shows the agent (avatar/color/name) + its latest work-point pulsing. Orchestrator's own current focus visible too (ties to §34 GOAL HUD / a presence strip).
- "Who's on what" glanceable: scan the board → see every active agent on its task.
- Honors §31 (no camera disruption on live presence updates — pulse / off-screen signal only).

## Gaps / surfaces to build
- `task take`/ownership already exist — verify they set last-author.
- Work-point author capture: ensure every `plan record` stamps author (pane/agent id) — verify/add.
- A `current focus` field per agent (or derive from last work-point) + CLI to set/show (`agent focus set/show` or fold into `plan record`).
- Reminder hooks: heartbeat template line, cli-hint on stale focus, cadence trigger.
- Canvas presence render (per-agent marker on owned task + orchestrator focus).
- Ownership-history view (`task history` / `plan related` over work-point authors).

## Meta (why now)
The orchestrator itself was NOT surfacing its current task — Andrey couldn't see what it was doing. This feature makes every agent's current work visible by construction + reminded, so the whole swarm is legible. Loops/passes (§35.7) compose: a re-opened task shows the new pass's worker taking it.
