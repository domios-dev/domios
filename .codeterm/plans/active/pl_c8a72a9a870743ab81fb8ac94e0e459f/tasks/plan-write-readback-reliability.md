---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_deeef71d6b0e429685940c6d867c5e09
handle: plan-write-readback-reliability
title: L6 plan-write reliability and read-back verification
status: done
---

Fix blind-OK-then-stale-read and database locked behavior for plan/task writes. Mutations that return OK must read back and verify the requested state, or surface an explicit busy/pending error. Never blind-retry database locked because the write may have succeeded; verify by reading back first.
