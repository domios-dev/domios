---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_dc7956fe684a419ab2c2a70071819a08
handle: worlds-v3-body-reveal-ms_dc7956fe684a419ab2c2a70071819a08
title: decision
---

§35.3 — made canvas-task-body a fixed description slot that always renders at W2+ (body markdown or empty-state). Root cause it was unreachable: all 101 dev-plan tasks have empty body and it was gated on bodyChars>0. Flipped worlds-v3 spec body-reveal from soft annotation to hard assert.
