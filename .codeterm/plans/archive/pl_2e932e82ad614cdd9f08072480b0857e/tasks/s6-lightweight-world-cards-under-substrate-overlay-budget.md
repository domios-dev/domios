---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_49da65ccc9314196bf83a734b9554bf8
handle: s6-lightweight-world-cards-under-substrate-overlay-budget
title: S6 — Lightweight world cards under substrate overlay budget
status: done
tags:
- canvas
- substrate
- performance
---

Implement the next performance slice after S5: under substrate/overlay budget flags, replace non-focused rich world cards with cheap label/skeleton cards so pan/zoom no longer pays full card chrome cost.

Context:
- S5 mounts focused rich overlays and suppresses rich bodies in the world layer, but the world layer still mounts full DOM card chrome.
- User-visible jank remains likely until non-focused nodes use a lightweight representation or passive-only rendering.

Scope:
1. Add a lightweight world-card representation for plan/task/spec nodes under `canvas.substrateV2=true` + `canvas.overlayBudget=true`.
2. The lightweight card should preserve fixed world footprint, node id/test hooks, labels/status/accent enough for navigation, and click/double-click/focus behavior.
3. Focused/selected nodes should still use `CanvasRichOverlayLayer` for rich content.
4. Keep the default non-flag behavior unchanged.
5. Add a measurable DOM-budget/debug hook/test: e.g. rich world card count = 0, lightweight world card count > 0, overlay count <= 2.

Acceptance:
- With substrate+overlay budget enabled, non-focused world plan/task/spec nodes do not mount full TaskCard/SectionNode chrome or rich bodies.
- Focused overlay still renders rich content at exact OverlayNode screenRect.
- Playwright can verify the budgeted mode on the seeded workspace canvas.
- Files remain under 1000 lines.
