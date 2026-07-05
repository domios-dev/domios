---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_09d95d960fbc46febc2280bb61f89dd2
handle: body-text-must-reveal-on-zoom-worlds-content-band-26-ms_09d95d960fbc46febc2280bb61f89dd2
title: testing
---

BUG FIXED (Andrey #1 no real text in the plan): deleted CanvasView.tsx contentNear=scale>=1.3 global flag AND SectionNode.tsx !isPlan body gate. SPEC section bodies, PLAN per-row markdown excerpts (W2) / fuller (W3), and TASK body + model_rationale + step cards all REVEAL on zoom — no titles-only, no click-toggles. RichMarkdown was already wired; it was a per-node threshold bug.
