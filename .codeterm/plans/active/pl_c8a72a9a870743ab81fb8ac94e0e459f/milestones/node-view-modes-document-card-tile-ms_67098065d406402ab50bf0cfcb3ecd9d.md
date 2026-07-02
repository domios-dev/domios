---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_67098065d406402ab50bf0cfcb3ecd9d
handle: node-view-modes-document-card-tile-ms_67098065d406402ab50bf0cfcb3ecd9d
title: status_update
---

[Mira/canvas-v3fe] Implemented three discrete designed view modes (Document/Card/Tile) for plan, spec-section and task nodes. Document=full markdown (RichMarkdown mermaid/code)+badges; Card=title+ready/count badges+1-2 line plain digest; Tile=title+status glyph+counts. Text FIXED readable size per mode — removed continuous counter-scale (.ct-canvas-node/title font curves + --ct-canvas-zoom var deleted). Switching mode swaps layout, not font scale.
