---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_acf6980706c34eae8524471fae91db3e
handle: bug-show-agent-action-does-not-reveal-tab
title: 'Bug: Show agent action does not reveal tab'
status: done
---

Bug from Andrey: the task action formerly labeled Open pane, now Show agent, is broken. Clicking it does not reveal/focus/show the agent tab. Investigate the full path from PlanDetailOverlay/DetailContent onOpenPane through PlanBoardView/PlanListView and workspace/pane focus APIs. Fix the behavior so clicking Show agent reliably focuses and reveals the assigned agent tab from Board/List/detail/slideover surfaces. Keep user-facing copy as Show agent, not Open pane. Add focused unit coverage and a Playwright/UI smoke if possible.
