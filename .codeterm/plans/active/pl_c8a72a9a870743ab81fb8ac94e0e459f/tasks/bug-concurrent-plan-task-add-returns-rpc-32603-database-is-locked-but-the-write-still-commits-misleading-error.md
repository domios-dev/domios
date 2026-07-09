---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_c79dd85b1ec947d7931b8466ad7b6794
handle: bug-concurrent-plan-task-add-returns-rpc-32603-database-is-locked-but-the-write-still-commits-misleading-error
title: 'BUG: concurrent ''plan task add'' returns RPC -32603 ''database is locked'' but the write still commits — misleading error'
status: done
tags:
- friction
---

Firing several 'plan task add' back-to-back, 3/4 returned -32603 'io error: database is locked' yet all 4 tasks landed (confirmed via plan get). Single-writer sqlite contention surfaces as a hard error on a successful-or-retryable write. Add write-retry/backoff or busy_timeout so concurrent plan writes don't error spuriously.
