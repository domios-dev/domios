---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_219c20e3ec244e7c9450d86a5bacabcc
handle: d1-design-spec-the-adapted-architecture-ms_219c20e3ec244e7c9450d86a5bacabcc
title: testing
---

ACCEPTED (orchestrator + supervisor). 403-line spec docs/specs/2026-06-25-canvas-substrate-design.md @de420b4a on canvas/d1-design. Converged arch baked, idle-node renderer correctly spike-gated on S0, capability map no-silent-drop, §6 migration M0-M5 task-cards-first behind canvas.substrateV2 flag (crispScale subsumed), §8 camera-stability migration-must-not-regress. Brief correction confirmed: world matrix already at CanvasView:670; offender is per-card crispScale at TaskCard:296.
