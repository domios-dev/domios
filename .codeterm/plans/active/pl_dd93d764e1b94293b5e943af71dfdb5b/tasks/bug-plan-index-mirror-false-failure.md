---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_354c755f6e36486eab4bfcc2351cf7aa
handle: bug-plan-index-mirror-false-failure
title: 'Bug: plan_index_mirror returns false failure after committed writes'
status: done
---

Investigate and fix repeated `plan_index_mirror is pending after a committed write` false failures. Observed on task add/update/heartbeat set after the data was actually committed. The desired behavior: either complete the mirror synchronously enough for the command to return OK, or return a non-fatal notice only when the primary write is committed and readback is safe. Do not hide real read/write ambiguity. Add focused tests around committed-write + mirror-pending behavior.

Dogfood evidence from this orchestration run:
- task add created the task but returned exit 1 with safe-readback advice.
- task update assigned pane/lane but returned exit 1; readback showed fields were committed.
- heartbeat set updated focus/lanes but returned exit 1.
- cleanup-status updates committed but returned exit 1.

This confuses agents into retrying or treating a successful write as failed. Keep the safe readback guidance for truly ambiguous writes, but do not make successful routine mutations fail hard because the mirror/index is still pending.
