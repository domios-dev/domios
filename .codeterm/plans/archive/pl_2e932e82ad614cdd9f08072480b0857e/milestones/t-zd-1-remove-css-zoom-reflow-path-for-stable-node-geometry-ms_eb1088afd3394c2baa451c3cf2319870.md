---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_eb1088afd3394c2baa451c3cf2319870
handle: t-zd-1-remove-css-zoom-reflow-path-for-stable-node-geometry-ms_eb1088afd3394c2baa451c3cf2319870
title: status_update
---

Accepted and integrated in hot checkout. Removed per-card CSS zoom/counter-scale wrappers from TaskCard and SectionNode so node geometry stays under a single camera transform. Gates: remote Canvas Orchestrator proved RED-before-GREEN Playwright drift probe on zd1:7697; local hot checkout passed npx tsx cards.test.tsx (20/20), nodeRegistry.test.tsx (5/5), npx tsc --noEmit, git diff --check. Residual worlds goldens are pre-existing and reproduced identically on old code.
