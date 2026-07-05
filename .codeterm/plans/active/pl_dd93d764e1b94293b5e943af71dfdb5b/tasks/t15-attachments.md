---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9e56d2683c07482588cd30a9b2237066
handle: t15-attachments
title: 'Image attachments: tasks, sections, reports + overlay thumbnails'
status: done
---

Image attachments for tasks, sections, strides/reports (spec section task-attachments). One shared attachment layer.

Steps:
1. Storage: attachments/ subtree in the plan's canonical sidecar dir (routed via canonical-sidecar path rules); refs (filename, caption, added_by, ts) on task/section/milestone records; 2MB-per-image soft cap (warn), reject >8MB.
2. CLI: plan task attach / spec section attach / plan section attach / agent report --attach; list in task show; delete with parent entity.
3. UI: thumbnails + click-to-full in the shared detail overlay (task/plan/spec kinds) and stride timeline.
4. Mesh: bytes route to the owning daemon for foreign plans (compose with homing + auth; enforce cap pre-transfer).
5. Doctor: orphaned attachment dirs flagged.
6. Tests: attach/list/delete round-trip, cap enforcement, canonical-path routing from a worker worktree, overlay thumbnail render test.
7. Dogfood: attach the planner evidence set (/tmp/planner-shots/10-List.png, 10-Canvas.png, 11-canvas-zoomout.png + Andrey's round-2 screenshots) to t10-list-v2 / t11-canvas-v2 as reference briefs — first real use.

Commands: narrowed cargo tests + npm run test:unit (overlay); ./build.sh check; ./build.sh cli; live attach smoke on dev.
Report: CLI surface, storage layout, overlay screenshots, dogfood attachment evidence.
