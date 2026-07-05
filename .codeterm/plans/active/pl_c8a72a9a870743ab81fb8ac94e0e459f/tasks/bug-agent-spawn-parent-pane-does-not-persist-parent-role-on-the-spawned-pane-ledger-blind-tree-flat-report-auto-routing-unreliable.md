---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_47ebbe90563e46de9231b88f4642d723
handle: bug-agent-spawn-parent-pane-does-not-persist-parent-role-on-the-spawned-pane-ledger-blind-tree-flat-report-auto-routing-unreliable
title: 'BUG: ''agent spawn --parent <pane>'' does not persist parent/role on the spawned pane (ledger blind, tree flat, report auto-routing unreliable)'
status: cancelled
tags:
- friction
---

Spawning workers with --parent fa0974b3 leaves the child pane at parent=None, role=None (confirmed by supervisor on panes 931e7289/cf5730cc). Consequences: 'agent ledger' shows no workers, pane-tree nesting is flat, and parent-based report auto-routing can't be relied on. Workaround: orchestrator tracks worker pane ids manually and each worker reports with an explicit 'codeterm agent report ... --pane <orchestrator>'. Fix: persist parent_pane + role atomically at spawn so ledger/tree/routing reflect the hierarchy.
