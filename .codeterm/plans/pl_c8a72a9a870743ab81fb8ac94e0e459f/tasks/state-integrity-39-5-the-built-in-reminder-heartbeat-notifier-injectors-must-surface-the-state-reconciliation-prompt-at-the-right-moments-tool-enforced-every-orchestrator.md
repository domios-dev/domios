---
tags:
- state-integrity
---

The CLI already injects orchestration reminders (who-you-are, autonomy/self-recovery) via task_725d2b5 (CLI+heartbeat+notifiers, done) + the evolving re-injected heartbeat task_a1d84645 (done). EXTEND them so they ALSO surface the STATE-RECONCILIATION reminder (see rule_orchestrator_state_reconciliation_routine + §39.1) at the RIGHT MOMENTS: after a worker report/close, on each heartbeat fire, and on idle. Goal: reconcile is TOOL-ENFORCED for EVERY orchestrator (reaches them like the who-you-are/autonomy reminders), not just private self-discipline. Links: §39.1 (skill side, task_99d8c406), §39.2 plan-reconcile doctor (task_47d64dfd), task_725d2b5, task_a1d84645.