---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_62d923d5c361480f9e97a961c7bd8da9
handle: manual-controls-scale-view-text-toggles-ms_62d923d5c361480f9e97a961c7bd8da9
title: status_update
---

Height-bound fix + §23.4 STEP 3 (419ead02). THE KM-STRIP FIX: board was ~20000px tall (fit-all 5%) because Document task cells (320px)+subtask super-cells stack when tasks are ~1/section. Replaced band-row sweep with ANCHOR-HUGGING SKYLINE column packing -> bounds field to ~ScellH/cols. Dogfood-scale (25 sec/45 tasks/22 subtasks): Tile world 1584px (was ~20000), Card 2352px -> fit-all ~50% not 5%. STEP 3: default mode=Tile overview + default mount=camera-focus on active plan (not fit-all); toolbar presets Overview(Tile+fit)/Read(Document+focus)+minimap toggle; dbl-click node -> camera-focus+lock Document+dim neighbours+breadcrumb (Esc back); minimap HUD with live viewport rect. tsc clean, all canvas suites green (skyline height-bound regression test, dbl-click breadcrumb test), vite clean. Deferred: SectionChunk>30 (dogfood has 25).
