---
tags:
- friction
---

Spawning workers with --parent fa0974b3 leaves the child pane at parent=None, role=None (confirmed by supervisor on panes 931e7289/cf5730cc). Consequences: 'agent ledger' shows no workers, pane-tree nesting is flat, and parent-based report auto-routing can't be relied on. Workaround: orchestrator tracks worker pane ids manually and each worker reports with an explicit 'codeterm agent report ... --pane <orchestrator>'. Fix: persist parent_pane + role atomically at spawn so ledger/tree/routing reflect the hierarchy.