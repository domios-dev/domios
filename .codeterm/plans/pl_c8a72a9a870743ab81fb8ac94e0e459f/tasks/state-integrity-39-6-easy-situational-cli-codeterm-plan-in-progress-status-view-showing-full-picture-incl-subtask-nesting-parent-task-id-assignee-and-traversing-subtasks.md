---
tags:
- state-integrity
---

Make seeing the live situation ONE easy command. 'codeterm plan in-progress [<plan>|--all]' lists every in_progress/in_review/blocked task with: status, PARENT/SUBTASK nesting (indent via parent_task_id), assignee pane+agent, branch, folder (see §39.7), and live-worker presence. The check MUST TRAVERSE SUBTASKS (a parent's situation includes its subtasks' states; today reconcile/views don't walk parent_task_id). Complements §39.2 doctor (this is the read-view). Bonus --json for the orchestrator.