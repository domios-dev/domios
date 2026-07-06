---
plan_id: pl_f75de79aca0c44f3b5af8a4e1ca4a35a
kind: task
id: task_45c50e0975ff4e0ba770bccd07aca8da
handle: hygiene-spec-headers-everywhere-gate-kill-sleep-waits-strengthen-weak-or
title: 'Hygiene: spec headers everywhere + gate, kill sleep-waits, strengthen weak oracles'
status: pending
---

Executes Gaps G1-G3 per the spec. (1) HEADER CONTRACT: every spec gets a 3-8 line header (WHAT / WHY / ORACLE — mirror the 13-canvas house style); the ~10 headerless files listed in Gaps G1 first; codename taxonomies (RAIL-4, GRP-4) defined in the header that uses them. ENFORCE: spec-header gate script (same family as the comment gate) — headerless spec fails CI. (2) SLEEP-WAITS: sweep 70 waitForTimeout to condition waits (expect.poll/waitForSelector); justified residue only with inline reason. (3) WEAK ORACLES: disposition every spec where toBeVisible/toBeTruthy is the sole final assertion — strengthen to behavioral or justify. Zero test semantics changed without recorded reason; golden suite green after.