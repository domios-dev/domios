---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_982bf856a061417798c7fd9314929385
handle: bug-sidebar-expanded-child-panes-hidden
title: 'bug: Sidebar hides expanded child panes from backend tree'
status: done
---

Bug observed during dogfood orchestration.

Symptom:
- Worker pane `58b78505 stride-preserve-save` is correctly present in backend/CLI hierarchy under parent pane `4d479b6c Puccini` in workspace `ef145fd1 codeterm`:
  `4d479b6c Puccini -> 58b78505 stride-preserve-save`.
- In the UI Sidebar, the parent pane can appear expanded but the nested child tab is not visible/rendered. The user saw a strange hidden tab inside the pane area.

Objective:
- Fix Sidebar/workspace tree rendering so child panes present in the backend pane tree are visible when the parent pane is expanded.
- Preserve explicit workspace placement and parent/child hierarchy semantics fixed by prior workspace-moved-tabs work.
- Add a focused test for the frontend projection/render helper or Sidebar component: a parent pane with a live child pane must render that child when expanded, even if the child was spawned by agent spawn with a cwd different from workspace cwd.

Strict non-goals:
- Do not change spawn/backend hierarchy unless the UI bug proves backend data is missing.
- Do not rename tabs automatically.
- Do not touch storage §33 work.

Acceptance:
- CLI/backend pane tree and Sidebar visible tree agree for parent `4d479b6c` with child `58b78505`-like data.
- Focused test fails before fix and passes after.
- Run the smallest relevant TS/React test; orchestrator will run warm gates.
