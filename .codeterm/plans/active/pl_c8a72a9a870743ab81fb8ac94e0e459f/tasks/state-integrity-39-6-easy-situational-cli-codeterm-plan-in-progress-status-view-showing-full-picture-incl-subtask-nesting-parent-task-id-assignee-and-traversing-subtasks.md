---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_2e2a60dd07a1457cacd0fd4c0748ad05
handle: state-integrity-39-6-easy-situational-cli-codeterm-plan-in-progress-status-view-showing-full-picture-incl-subtask-nesting-parent-task-id-assignee-and-traversing-subtasks
title: 'State-integrity §39.6: easy situational CLI — ''codeterm plan in-progress'' (+status view) showing FULL picture incl SUBTASK nesting (parent_task_id), assignee, and traversing subtasks'
status: done
tags:
- state-integrity
---

Make seeing the live situation ONE easy command. 'codeterm plan in-progress [<plan>|--all]' lists every in_progress/in_review/blocked task with: status, PARENT/SUBTASK nesting (indent via parent_task_id), assignee pane+agent, branch, folder (see §39.7), and live-worker presence. The check MUST TRAVERSE SUBTASKS (a parent's situation includes its subtasks' states; today reconcile/views don't walk parent_task_id). Complements §39.2 doctor (this is the read-view). Bonus --json for the orchestrator.
