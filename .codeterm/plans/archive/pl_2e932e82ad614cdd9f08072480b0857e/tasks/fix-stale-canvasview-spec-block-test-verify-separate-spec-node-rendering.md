---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_58c8c022476c4c8e8831342f8d50a704
handle: fix-stale-canvasview-spec-block-test-verify-separate-spec-node-rendering
title: 'fix: stale CanvasView spec-block test + verify separate spec-node rendering'
status: pending
---

CanvasView.test.tsx '§24.3 renders SEPARATE spec + plan nodes' fails on HEAD (pre-existing, not from the design loop): asserts [data-testid=canvas-spec-block] which the worlds-LOD SectionNode migration appears to have renamed/restructured. Needs the running canvas to determine: (a) do spec nodes still render as separate blocks (testid renamed -> update test), or (b) did separate-spec-node rendering regress (real fix)? Diagnose with dev :7691 + Playwright. Tracked debt; schedule in the consistency/sweep iter (iter18).
