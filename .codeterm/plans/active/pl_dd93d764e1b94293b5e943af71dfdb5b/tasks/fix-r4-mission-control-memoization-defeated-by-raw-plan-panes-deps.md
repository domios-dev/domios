---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_65bc9d678579413c99d782cd21b8926a
handle: fix-r4-mission-control-memoization-defeated-by-raw-plan-panes-deps
title: 'Fix R4: Mission Control memoization defeated by raw plan/panes deps'
status: done
---

HIGH review finding from R4 Canvas frontend surfaces. MissionControlLiveView computes missionControlInputRevision at src/components/Canvas/missionControl/MissionControlLiveView.tsx:40, but the expensive memos still depend on raw plan/panes identities at lines 54, 55-64, and 67. Because PlanViewShell keeps canvas/list/board mounted hidden (src/components/Canvas/PlanViewShell.tsx:83-104), pane-store identity churn or parent renders can rebuild buildMissionControl/modelToSnapshot/selectActiveWork for the hidden canvas surface even when missionControlInputRevision is unchanged. Fix by making the revision key the semantic dependency for expensive builders: derive stable inputs keyed by missionControlInputRevision, remove raw plan/panes from deps where safe, or split the revision into plan/presence keys and pass only stable callbacks/maps needed by modelToSnapshot. Also audit PlanViewShell callers for newly allocated props that defeat memo surfaces.
