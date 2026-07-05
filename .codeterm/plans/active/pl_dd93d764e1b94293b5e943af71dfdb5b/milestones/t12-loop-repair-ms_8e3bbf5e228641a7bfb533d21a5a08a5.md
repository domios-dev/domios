---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_8e3bbf5e228641a7bfb533d21a5a08a5
handle: t12-loop-repair-ms_8e3bbf5e228641a7bfb533d21a5a08a5
title: decision
---

trace complete: record_iteration is only called from PlanStore::advance_loop, which is only exposed through manual plan.loop.advance RPC/REST/CLI entrypoints; no live orchestration/heartbeat/replan workflow records loop iterations automatically today
