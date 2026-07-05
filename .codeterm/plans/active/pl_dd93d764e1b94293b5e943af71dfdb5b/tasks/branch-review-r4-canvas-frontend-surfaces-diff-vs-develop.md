---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_c584c2a6df71456481f4e91c2f365df6
handle: branch-review-r4-canvas-frontend-surfaces-diff-vs-develop
title: 'Branch review R4: Canvas frontend surfaces (diff vs develop)'
status: done
---

READ-ONLY REVIEW R4 Canvas frontend surfaces. Findings: 2 HIGH. Finding 1: MissionControlLiveView computes missionControlInputRevision but buildMissionControl/modelToSnapshot/selectActiveWork use useMemo deps that still include raw plan and panes identities, so pane-store identity churn and parent renders recompute the expensive model for the hidden mounted canvas surface. Finding 2: PlanListView forces sections open when statusFilter !== all or textQuery is set; selecting Done renders every done row, so a 200+ done-task plan produces unbounded DOM despite the default collapsed done rollup. Product code untouched.
