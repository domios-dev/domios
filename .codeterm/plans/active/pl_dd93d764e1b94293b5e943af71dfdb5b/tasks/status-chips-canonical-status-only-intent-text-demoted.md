---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_16d82c867c5b4393b8276df3d5d8bdff
handle: status-chips-canonical-status-only-intent-text-demoted
title: 'Status chips: canonical status only, intent text demoted'
status: done
---

## What

STATUS chips on board cards and in the task slide-over currently render free-form intent/decision text ("activeWork selector landed; ready to dispatch", "Active: QA Pipeline Mechanics") instead of the canonical task status. Evidence: user screenshots — slide-over STATUS chip shows a decision sentence; board card pill shows a truncated lane label.

## Fix

1. Locate the mapping that feeds the STATUS chip / card status pill (suspects: boardModel deriving a label from the latest decision milestone or the active execution-attempt label). STATUS renders ONLY the canonical TaskStatus (pending / in progress / in review [+ In QA chip] / blocked / parked / done / cancelled) with the standard status colors.
2. The contextual text is still valuable — demote it: on cards, an optional single muted "latest note" line at the card bottom (truncated, small type); in the slide-over, it lives in the Activity timeline (it already is a milestone there — no duplication in the header).
3. Status chip and note line come from different fields with different testids (status-chip, latest-note) so this class of leak is test-locked.

## Files

src/components/Canvas/planView/boardModel.ts (+tests), PlanBoardView, slide-over/detail header component, milestoneColors/status color module.
