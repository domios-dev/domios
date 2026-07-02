---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9d66a17ebded4c14971e26114d033c45
handle: card-footprint-fixed-zoom-ms_9d66a17ebded4c14971e26114d033c45
title: decision
---

root-caused footprint drift (§22.1) to missing transform-origin on the crisp zoom wrapper — default center origin shifts the box by (k-1)/2·size; set origin 0,0 for exact identity. Extended crispScale re-rasterization to TaskCard (threaded via nodeRegistry); StrideSnake inherits crisp as a child. testing: tsc --noEmit clean for all Canvas files (only pre-existing plugin-sdk resolution noise remains).
