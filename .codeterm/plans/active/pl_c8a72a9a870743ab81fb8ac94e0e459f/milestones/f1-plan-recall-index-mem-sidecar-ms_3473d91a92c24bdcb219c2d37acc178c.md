---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_3473d91a92c24bdcb219c2d37acc178c
handle: f1-plan-recall-index-mem-sidecar-ms_3473d91a92c24bdcb219c2d37acc178c
title: decision
---

ANDREY: reuse/EXTEND existing code (mem engine, plan watcher), NOT rebuild. NO legacy, NO band-aids — do it right. Decomposed into F1a-f. Backfill+recall test on the REAL dogfood plan (pl_c8a72a9a, 7686) is the acceptance gate — orchestrator runs it at integration (worker builds+TDD in isolated 7694).
