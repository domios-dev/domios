---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_8a5c9870b6c842db863f2ff62e63e76c
handle: mira-frontend-rebuild-ms_8a5c9870b6c842db863f2ff62e63e76c
title: status_update
---

[status_update] §22/§23 STEP 1 landed (0f72261b): ONE fixed-size SectionedPlanNode merging plan+spec. HEADER (title/owner/coverage + per-node Doc|Card|Tile switch + fit-node) + always-rendered RIGHT-EDGE RAIL (per section: glyph·title·[n]·OUTPUT dot = in-node TOC + stable task anchors) + BODY in FIXED frame per manual mode (Document=full RichMarkdown of section bodies w/ INTERNAL SCROLL; Card=titles+digests; Tile=coverage summary). Spec prose = section bodies (no separate spec node). KILLED auto-by-zoom (mode is manual; zoom=camera only). FIXED bbox per mode: planTileH 280/planCardH 400/planDocH 640, width 480, railW 100. Fixes the 3 live bugs: (1) overflow -> nodeStyle now FIXED height, content scrolls inside frame; (2) plan-as-title-list -> rail+section bodies; (3) milestone text -> Document interior (step cards next, STEP 2). Tasks band off rail OUTPUT dots (§20.3 sweep). intraPlan 164->92. tsc clean, all canvas suites green (added cards/sectionedPlanNode.test.tsx), vite build clean, cargo+clippy pass.
