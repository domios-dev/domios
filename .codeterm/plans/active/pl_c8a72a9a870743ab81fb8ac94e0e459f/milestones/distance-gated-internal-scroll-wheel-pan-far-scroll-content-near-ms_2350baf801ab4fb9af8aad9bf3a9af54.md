---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_2350baf801ab4fb9af8aad9bf3a9af54
handle: distance-gated-internal-scroll-wheel-pan-far-scroll-content-near-ms_2350baf801ab4fb9af8aad9bf3a9af54
title: decision
---

ANDREY VISUAL FEEDBACK (STEP 1 board): (1) task layout still bad — grows straight DOWN as one tall column; banded multi-column is the fix and is PRIORITY #1 (band each task next to its section's rail OUTPUT). (2) Couldn't see section→task connections because tasks are a far column — banded resolves this; spec is now the section BODIES inside the ONE node (§22.2 merge, correct). (3) Internal scroll idea APPROVED, but must be DISTANCE-GATED (§23.6): far = wheel pans canvas (no intercept), near/focused (camera>~1.0) = wheel scrolls node content, release at boundary, don't annoy. Iterate: STEP2 banded → screenshot+review → STEP3 + scroll-gating.
