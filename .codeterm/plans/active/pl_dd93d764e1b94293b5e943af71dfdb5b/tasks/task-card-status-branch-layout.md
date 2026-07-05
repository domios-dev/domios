---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_57df061811f1409d99c2aa852ac252f6
handle: task-card-status-branch-layout
title: Task card status/branch row layout
status: done
---

Small UI polish from Andrey screenshot: task cards currently place status text/chip and branch/lane chip on the same line, so long branch names can push/squeeze the status and make the card look broken. Fix the task card layout so status has its own stable row and branch/lane metadata is rendered on a separate row below it. Preserve dense board/list aesthetics, avoid oversized spacing, and keep text truncation professional at narrow card widths.

Likely area: src/components/Canvas/planView/BoardTaskCard.tsx, board card model/styles, and any shared task-card styles/tests. Do not change task semantics. Keep user-facing labels clean.
