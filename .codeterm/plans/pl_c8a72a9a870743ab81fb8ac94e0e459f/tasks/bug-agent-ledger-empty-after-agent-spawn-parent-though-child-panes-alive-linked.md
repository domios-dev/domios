---
tags:
- friction
---

Spawned 2 workers with --parent fa0974b3 on prod 7685; both panes alive (pane status ok), set-orchestrator ok, but 'codeterm agent ledger --pane fa0974b3' reports 'No workers in ledger'. Durable ledger not reflecting the parent link → orchestrator tracking view is blind even though report routing works. Investigate spawn→ledger write path.