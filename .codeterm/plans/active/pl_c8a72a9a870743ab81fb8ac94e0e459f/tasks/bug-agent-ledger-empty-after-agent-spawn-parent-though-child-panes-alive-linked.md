---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_db061cb2e3ab4abb8c3afc9e4f7db16a
handle: bug-agent-ledger-empty-after-agent-spawn-parent-though-child-panes-alive-linked
title: 'BUG: agent ledger empty after ''agent spawn --parent'' though child panes alive + linked'
status: done
tags:
- friction
---

Spawned 2 workers with --parent fa0974b3 on prod 7685; both panes alive (pane status ok), set-orchestrator ok, but 'codeterm agent ledger --pane fa0974b3' reports 'No workers in ledger'. Durable ledger not reflecting the parent link → orchestrator tracking view is blind even though report routing works. Investigate spawn→ledger write path.
