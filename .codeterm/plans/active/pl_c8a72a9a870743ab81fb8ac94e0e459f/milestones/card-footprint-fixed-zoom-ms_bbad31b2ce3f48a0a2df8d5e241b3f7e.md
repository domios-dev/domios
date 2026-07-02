---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_bbad31b2ce3f48a0a2df8d5e241b3f7e
handle: card-footprint-fixed-zoom-ms_bbad31b2ce3f48a0a2df8d5e241b3f7e
title: decision
---

SectionNode reflow root was NOT frame (already fixed-constant) — it was the spec BODY clamp toggling (excerpt@W2→full@W3 re-layout). Fix clamp=isPlan: spec renders deepest content the moment it reveals, W2→W3 only re-rasterizes. My brief misdiagnosed root cause (lesson: briefs point at symptom+file, not assert mechanism). PENDING Andrey visual: spec body no longer reflows on zoom
