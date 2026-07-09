---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4ab4c21310ac4b8b927d8de80b4c3260
handle: plugin-extensible-canvas-nodes-ms_4ab4c21310ac4b8b927d8de80b4c3260
title: decision
---

ANDREY: canvas OPEN FOR PLUGINS — plugin creates EXTERNAL node kinds (GitHub node etc), visible on canvas, AGENTS query via CLI. FOUNDATION EXISTS: FE nodeRegistry.registerNodeRenderer(kind,renderer)+open-kind fallback; BE CanvasElement{kind:String}+Plan.elements. GAPS: (1) plugin API to register node kind+renderer; (2) plugins create/update CanvasElement via batch add_element/host API; (3) agent CLI codeterm canvas nodes list|show; (4) capability+JS parity. Honors plugin boundary: core asks capabilities not identities.
