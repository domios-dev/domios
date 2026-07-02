---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_333babae0dcf4d849fcc2ffe3cc2cee3
handle: node-view-modes-document-card-tile-ms_333babae0dcf4d849fcc2ffe3cc2cee3
title: status_update
---

[status_update] §23.2 STEP 2 landed (eb15b584): fixed task-cell sizes per mode (Tile 200x72/Card 260x120/Doc 260x320+bounded step interior) — banded <=3-col grid right of the plan node, top-down non-overlap, edges bezier from rail OUTPUT dot -> task. Thread&Coil steps now INTERIOR to the card (no separate milestone nodes): Tile=dots, Card=dots+count, Document=step CARDS with milestone TEXT+iteration (fixes 'steps are just dots' bug). NEW §23.6 distance-gated internal scroll: node body only intercepts wheel when zoomed IN (scale>=1) AND can scroll; at boundary or zoomed out -> canvas pans (no trap). Removed the always-on stopPropagation that fought panning. tsc clean, 11 canvas suites green (TaskSteps per-mode, fixed cell w/h, interior-steps layout, 3 scroll-gate cases), vite clean. Banded grid kills the tall column + makes section->task legible. STEP 3 next (mount focus@90%, presets, dbl-click focus+dim+breadcrumb, minimap, SectionChunk>30).
