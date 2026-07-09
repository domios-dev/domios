---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_fd771a43172c402988c7f230897748a2
handle: state-integrity-39-5-the-built-in-reminder-heartbeat-notifier-injectors-must-surface-the-state-reconciliation-prompt-at-the-right-moments-tool-enforced-every-orchestrator-ms_fd771a43172c402988c7f230897748a2
title: status_update
---

Integrated state-reconcile-injectors: shared STATE-RECONCILIATION prompt is prepended for active-plan orchestrator heartbeat/delayed-send composition, in_review owner notifications, and orchestrator plan reminders while worker reminders and normal sends stay untouched. Verified cargo fmt --check, git diff --check, heartbeat_tests 3/3, reminders tests 5/5, and CODETERM_API_PORT=7686 ./build.sh check.
