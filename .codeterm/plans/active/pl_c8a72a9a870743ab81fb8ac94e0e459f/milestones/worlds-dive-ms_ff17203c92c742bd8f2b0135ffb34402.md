---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_ff17203c92c742bd8f2b0135ffb34402
handle: worlds-dive-ms_ff17203c92c742bd8f2b0135ffb34402
title: testing
---

worlds-dive deployed (per-band reveal W0→W3, legible titles, criterion fit→1000% with footprint/no-overlap/increasing-detail). STUCK: @canvas-worlds-v3 fails at body-reveal — canvas-task-body (TaskCard.tsx:268, gated showBodySlot=atLeast(world,W2)) not seen by visible()(>2px) after zoom. 2 fixes didn't crack it. NEXT (post token-reset): DOM-debug whether element is present-but-0px OR world prop never reaches W2 at the test's scales; appW=footprint(~156)×scale 2.3=359>W2(300) so it SHOULD be W2 — instrument the live world prop + body box height
