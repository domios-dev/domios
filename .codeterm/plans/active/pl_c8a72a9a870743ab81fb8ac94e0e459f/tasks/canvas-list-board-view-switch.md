---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_75d5e8ad42e24e6dbba4fff2a28cf3fc
handle: canvas-list-board-view-switch
title: Canvas/List/Board view switch
status: done
---

Add a calm segmented control at the top of the plan/canvas surface with modes Canvas, List, Board. Canvas keeps existing behavior. List and Board route to new read-only components. Persist mode per local UI state if existing canvas UI state pattern supports it, but avoid schema churn. Must work for workspace plan entry and pane plan overlay/open paths. No drag/drop editing.