---
plan_id: pl_f75de79aca0c44f3b5af8a4e1ca4a35a
kind: task
id: task_7abb573ccd464a23a4888dfcf149ab07
handle: visual-baselines-checked-in-goldens-for-key-surfaces-reviewed-regenerati
title: 'Visual baselines: checked-in goldens for key surfaces, reviewed regeneration only'
status: pending
---

Executes P4 of the spec — the highest-payoff stage: the user is currently the visual regression detector (Gaps G4). toHaveScreenshot goldens for: Board (seeded plan), List, Canvas fit-zoom, sidebar with seeded pair tree, task detail — fixed fixture + viewport, web-mode chromium only, tight maxDiffPixels. Baselines regenerate ONLY via an explicit script producing a reviewable diff — a lane cannot silently move a golden.