---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_cc56915fc3674447b70200b5039542a8
handle: node-view-modes-document-card-tile-ms_cc56915fc3674447b70200b5039542a8
title: status_update
---

[DECISION] Layout is mode-aware so a node reserves EXACTLY its mode's bounded frame -> no empty shells, no overflow by construction (proven by layout/viewModes.test.ts). Fixed heights: tile 56, planCard 132, specCard 92, taskCard 104; document = content-fit estimate. Plan-row anchors collapse to node mid-edges in Card/Tile (no rows there) so spec/task edges never float. Auto-by-distance bands: far(<0.45)->Tile, mid->Card, near(>=0.85)->Document — document threshold lowered from LOD's 1.1 so the reading view appears at ~100% not only past 110%. Text-off FORCES Tile (deliberate declutter, never an accidental empty box). Default setting = auto, user-lockable. Per-node override map (nodeModeOverrides + modeFor) is plumbed but no per-node UI yet — global is the right default and per-node wasn't cheap given layout coupling.
