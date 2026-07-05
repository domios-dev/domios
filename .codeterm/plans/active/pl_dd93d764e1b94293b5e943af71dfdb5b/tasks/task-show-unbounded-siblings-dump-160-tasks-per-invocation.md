---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9c13aa8c6d9b456a8d49618e9b53cdda
handle: task-show-unbounded-siblings-dump-160-tasks-per-invocation
title: 'task show: unbounded siblings dump — 160 tasks per invocation'
status: in_progress
---

USER-SPOTTED (round-64 screenshot): 'codeterm task show <id>' unconditionally renders a siblings: section listing EVERY task in the plan with full name + full id — on the live 160-task plan that is ~14KB of wall-text per invocation, drowning the actual task and burning agent context on every single task inspection (agents run task show constantly; this is a per-call token tax on the whole fleet). FIX the output contract: (1) DEFAULT: no sibling enumeration — one summary line instead: 'siblings: 160 in plan (12 open) — use --siblings to list'; (2) --siblings flag lists OPEN siblings only (pending/in_progress/in_review/blocked), each as short-id + status + truncated name (60 chars) — done/cancelled siblings appear only with --all; (3) same bounding for any other unbounded lists in task show (check attachments/subtasks render paths while there); (4) --json keeps full data (machine consumers unaffected) — this is a HUMAN/agent-text-output fix only. Same bounded-list discipline as the file-size rule: no CLI text output should scale linearly with plan size unless explicitly requested.
