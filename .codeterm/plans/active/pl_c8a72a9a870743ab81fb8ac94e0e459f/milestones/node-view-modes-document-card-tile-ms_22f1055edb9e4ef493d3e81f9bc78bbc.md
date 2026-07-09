---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_22f1055edb9e4ef493d3e81f9bc78bbc
handle: node-view-modes-document-card-tile-ms_22f1055edb9e4ef493d3e81f9bc78bbc
title: status_update
---

[status_update] §20.3 Banded Multi-Column task layout landed (cc6d6b93). Replaced the single tall task column (the портянka) with a bounded grid anchored to section rows: tasks band by primary covered section, wrap across <=3 cols, top-down non-overlap sweep hugs each band to its row Y. Subtasks=super-cell; step clusters now FOLD BELOW the card as interior height (uniform taskWidth cells -> clean grid, no horizontal sprawl). On a 38-task dogfood-scale fixture: height ~1412 vs ~2812 single-col, <=3 cols, zero overlap, deterministic. New layout/taskBands.ts; intraPlan shrank to 164 lines. Makes the view-modes readable (the 13% fit-to-view portянka is gone). tsc clean, all canvas suites green, cargo+clippy pass.
