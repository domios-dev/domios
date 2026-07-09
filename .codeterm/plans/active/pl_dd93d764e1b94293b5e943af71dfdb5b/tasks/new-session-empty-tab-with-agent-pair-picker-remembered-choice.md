---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_685b516678994f74bcdcce093649e351
handle: new-session-empty-tab-with-agent-pair-picker-remembered-choice
title: 'New session: empty tab with agent/pair picker + remembered choice'
status: done
---

## What

"New session" stops spawning a shell directly: it creates an EMPTY TAB whose content is an in-tab spawn picker. The user chooses (a) a single agent (provider cards) or (b) the **Orchestration pair preset** — Orchestrator + Planner spawned together as a linked pair (reuses create_orchestration_pair from paired-creation, already merged). Terminal-only tab stays available as a quiet option.

## UX (in the plan-view/settings visual language — quiet, CSS-var driven)

- Empty tab renders a centered picker: top row of provider cards (glyph + name + availability), below them ONE highlighted "Orchestration" card — visually a BRACKET/link joining two EQUAL rows: [Orchestrator: provider/model select] ⟷ [Planner: provider/model select]. **HARD REQUIREMENT (user, twice): the two rows are the SAME visual level — same size, same styling, joined by a pair affordance (bracket/link line). The Planner is NOT nested under, smaller than, or subordinate to the Orchestrator.**
- Defaults: Orchestrator = claude opus, Planner = claude fable — but **the whole last choice (mode + providers + models) is persisted** and pre-selected next time (reuse the existing persisted-UI-prefs mechanism from board-first view persistence; key ui.new_session.last_choice).
- Model options derive from provider descriptors (capability discipline, never hardcode model lists).
- Confirm on single agent -> spawns into this tab. Confirm on pair -> create_orchestration_pair; this tab becomes the Orchestrator pane, Planner opens as its sibling tab; both onboarded via PEER_ constants (already wired).
- Esc/blank state: the empty tab is a normal tab (closable); picker persists until a choice is made.

## Files

New NewSessionPicker component (+persisted-choice module with unit tests); sidebar New-session click handler; wire to existing OrchestrationCreate internals from Dali's lane (do not duplicate the form logic — extract/share); spawn plumbing already exists.
