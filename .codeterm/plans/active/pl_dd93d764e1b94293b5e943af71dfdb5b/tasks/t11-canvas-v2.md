---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e5310c3750414ba5b0d2ac997c46b73f
handle: t11-canvas-v2
title: 'Canvas v2: zoom clamps + dense packing + LOD retune'
status: done
---

Canvas v3 — REWORKED after Planner live inspection (round 3; round-2 'tighter packing' was insufficient — the LAYOUT ALGORITHM is the root failure). Read the ROUND 3 block of spec section "Canvas — tertiary role and performance constraints" FIRST. Evidence: /tmp/planner-shots/10-Canvas.png (diagonal scatter, unreadable card confetti) and 11-canvas-zoomout.png (BUG: at 47% zoom the viewport is empty, content off-screen, visible only in minimap).

Scope:
1. Zone-first layout: sections render as visible rounded zone containers (extend CanvasSectionZones into the primary structure) in deterministic column/grid flow; tasks pack inside zones in uniform tight grids; done-stacks inside zones; fixed gutters. Replaces the scatter layout.
2. Uniform cards: fixed width, 1-2 discrete heights, max two accents (status edge + presence dot); remove stacked mini-pill rows.
3. Camera correctness (P0 within this task): initial view AND Overview = fit-all-zones with margin; zoom-out clamps at fit-minus-margin so content can NEVER fully leave the viewport; limits relative to content bounds (fit..400%). Reproduce + fix the off-screen-at-47% bug; add a viewport test for it.
4. Edges faint/orthogonal, full opacity on hover/selection; subtle dot-grid background; LOD keyed to on-screen card size (no dot-cloud).
5. Frame budget re-verified (p95 ~16ms, 210-task fixture); BEFORE/AFTER screenshots at fit/100%/400% attached to report.
Commands: npm run test:unit; manual fixture sweep with frame probe; web-mode screenshots.
