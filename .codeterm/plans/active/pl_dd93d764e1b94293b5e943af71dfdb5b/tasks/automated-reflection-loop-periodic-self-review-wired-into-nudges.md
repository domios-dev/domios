---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_a92415d6b19e4bf6b7aa30473fee17b4
handle: automated-reflection-loop-periodic-self-review-wired-into-nudges
title: 'Automated reflection loop: periodic self-review wired into nudges'
status: done
---

## What

Automate the self-improvement loop the user asked for ("этот процесс можно автоматизировать и улучшать на лету"). The primitives all exist — heartbeat learnings (`codeterm orchestrator learn`, re-injected into every nudge), plan skills with keep/discard ledgers, the completion retrospective — but REFLECTION ITSELF is ad-hoc: it happened because the user asked. Make it periodic and evidence-driven.

## Design (wire existing primitives, invent nothing)

1. **Reflection trigger**: piggyback the milestone-gates mechanism — when a task GROUP closes (and additionally every N gate/bounce events, config `reflect.every_gates` default 15), the orchestrator's next nudge appends a one-time reflection block (shared constant): "Reflection point: review your last window — bounces issued and received, gates that later leaked (user-caught bugs on accepted lanes), incidents. For each lesson: durable rule -> `codeterm orchestrator learn`; reusable procedure -> `codeterm plan skill new/edit`; prompt-change proposal -> route to your Planner. Keep/discard honestly — discarded approaches are data."
2. **Evidence feed**: the block includes auto-computed counts for the window (tasks closed, bounces, gate-leaks if derivable from reopened/bounced-after-accept transitions) so reflection starts from numbers, not vibes.
3. **Two prompt lines from Puccini's own reflection (pinned, orchestrator_suffix)**:
   - "Inspecting or prototyping? Scratch worktree — the integration checkout stays clean; a dirty checkout is a gate hazard."
   - "An accepted lane with deferred/skipped evidence is OPEN RISK: track it as such until the evidence lands or the user explicitly accepts the substitute."
4. **Planner symmetry**: planner_suffix gets one line — at reflection points, review which plan skills earned promotion to machine mem and whether task/test-plan quality caused bounces.

## Executable test plan

1. Unit: reflection-trigger detection (group close fires once; N-gates counter fires and resets; both re-arm correctly).
2. Render test: the nudge block appears exactly once per trigger with the window counts (fixture plan with seeded events).
3. Prompt wiring tests reference the new constants (dedup rule).
4. Dev round-trip: seed a group close, observe the reflection nudge in the orchestrator pane, run `orchestrator learn`, verify the learning renders in the next heartbeat.
