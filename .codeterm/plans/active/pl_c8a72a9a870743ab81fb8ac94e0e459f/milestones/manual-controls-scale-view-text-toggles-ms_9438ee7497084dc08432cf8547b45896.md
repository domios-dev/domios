---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9438ee7497084dc08432cf8547b45896
handle: manual-controls-scale-view-text-toggles-ms_9438ee7497084dc08432cf8547b45896
title: status_update
---

[status_update] Added CanvasToolbar (top-center pill): explicit zoom in/out/fit/reset buttons + live zoom %, view-mode segmented switch (Auto/Doc/Card/Tile), text on/off toggle (off forces Tile). Wired via useCanvasViewport (zoomBy/fitToView/setTransform). Fit=fit-all bounds, Reset=100% centered. Mode switch + text toggle drive the global resolved mode that sizes the layout. Positioned clear of the bottom-center composer.
