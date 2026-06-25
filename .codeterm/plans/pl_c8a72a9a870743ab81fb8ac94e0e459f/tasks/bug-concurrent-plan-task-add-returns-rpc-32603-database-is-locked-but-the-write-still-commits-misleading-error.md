---
tags:
- friction
---

Firing several 'plan task add' back-to-back, 3/4 returned -32603 'io error: database is locked' yet all 4 tasks landed (confirmed via plan get). Single-writer sqlite contention surfaces as a hard error on a successful-or-retryable write. Add write-retry/backoff or busy_timeout so concurrent plan writes don't error spuriously.