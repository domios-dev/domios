---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_b18a137bea6540e5870ed8be599100a2
handle: d1-design-spec-the-adapted-architecture-ms_b18a137bea6540e5870ed8be599100a2
title: decision
---

SYNTHESIS of R1+R2 for D1. CONVERGED (bake into spec regardless): (1) ONE camera matrix on ONE world container, never per-card transforms = the #1 fix; (2) focused/editing/selection/IME/copy -> thin DOM overlay mounted at cameraMatrix*objectRect, demote-on-pan v1; (3) hidden DOM a11y proxy mirror from day one (opacity:0 not display:none), shares overlay interface. OPEN QUESTION (spike-gated, do NOT guess in design): idle/mid-node renderer = DOM-under-single-transform (R1) vs GPU MSDF-atlas+own-layout (R2). Discriminator = does single-container CSS matrix() keep DOM text crisp at 1000%? R2 says no (cites crispScale hack nodeRegistry.tsx:40), R1 says yes. S1 spike must answer THIS FIRST. Far/birds-eye band = cheap placeholder/canvas-label either way (both agree).
