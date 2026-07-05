---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_5ebae684ea7342cfbcd304f77dd495df
handle: canvas-mission-control-design-polish-pass
title: 'Canvas mission control: design polish pass'
status: done
---

## What

Design-polish pass over the LIVE Canvas mission control (merged at 42752793, in_review pending visual gate). The assembly line works mechanically; this task makes it feel crafted. Frontend-designer discipline: distinctive, quiet CodeTerm palette, no generic dashboard aesthetics.

## Polish targets (seed list — the visual gate findings extend it)

1. **Zone rhythm:** consistent spacing/scale ratio between dispatch rail, work floor bays, QA bay, gate desk, done shelf; the work floor must read as the visual center of gravity at every zoom step.
2. **Avatar life:** docked avatars use the shared liveliness grammar (breathing when Working, amber dim when idle >N, red pulse leftward travel on QA fail); transitions eased 300-500ms, compositor-friendly, reduced-motion static.
3. **Unhealthy-state legibility (design thesis):** empty work floor + non-empty dispatch must LOOK like a hole (test with squint): subtle empty-bay outlines, dispatch rail swell. QA queue >2 visually swells the bay.
4. **LOD polish:** far-out departure-board silhouettes readable (zone name + count only); mid-zoom bay cards never truncate titles into fragments (reuse the card-title fix lessons).
5. **HUD:** corner plan HUD compact, monospace numerals, no chart-junk.
6. Typography/边 details: title truncation word-boundary, consistent radius/border tokens, dark+light themes.

## Evidence-driven

Input: visual-gate screenshots from the live dogfood run + Andrey's gate feedback (loop 3/3). Output: before/after screenshots per polish item in the report.
