---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_0f2c369dea9044fd85606edbd19df790
handle: plan-goal-north-star-ms_0f2c369dea9044fd85606edbd19df790
title: decision
---

GOAL data model: add first-class Plan.goal meta, not a section/task. Shape: {vision_md, vector_md, key_constraints[], postulates[], spec_refs[], success_oracles[], updated_at, updated_by}. It is the north-star for the whole effort, never decomposed, never assigned, never counted as coverage; tasks/sections may cite it but do not own it. Recommendation: store in plan meta now, later in .codeterm/plans/<id>/meta.yaml goal block under git-native storage.
