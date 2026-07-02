---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8a84597c762c4828b4ae05c5403aecb5
handle: t2b-spec-pres
title: Spec card + spec read surface redesign
status: pending
---

Redesign spec entity presentation: the Canvas spec card and the spec read surface (user screenshot evidence shows truncated section titles, identical "-> tasks" chips, dead space).

Steps:
1. canvasRender/cardContent.ts: new spec card content model — spec title, section count, coverage rollup (covered/partial/unplanned counts), readable top-level section titles (wrap or truncate-with-meaning, min useful width); drop the per-row identical "-> tasks" chip; per-row coverage glyph + task count only when nonzero/informative.
2. canvasRender/drawCard.ts: render the new spec card model; keep card sizing sane (no large dead regions).
3. DetailContent spec branch (+ PlanDetailOverlay kind="spec" from task t2-detail-overlay): full untruncated section tree, expandable bodies, per-section linked plan sections and task coverage.
4. Update cardContent.test.ts + drawCard.test.ts for the new model.

Depends on t2-detail-overlay for the overlay shell (the card-content work can start immediately).
Commands: npm run test:unit; manual dev screenshot of a spec card at 100% and overview zoom.
Report: before/after screenshot paths, content model description, test output.
