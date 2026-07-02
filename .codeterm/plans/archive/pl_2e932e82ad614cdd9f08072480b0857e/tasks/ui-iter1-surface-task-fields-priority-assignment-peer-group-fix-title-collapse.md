---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_c45b40916e4c4e13b39b1af1f84eef17
handle: ui-iter1-surface-task-fields-priority-assignment-peer-group-fix-title-collapse
title: UI iter1 — surface task fields (priority/assignment/peer/group) + fix title collapse
status: done
---

Canvas UI/UX design loop, iteration 1/10 (committed 34123b8b on feat/canvas-perf-polish). Surfaced previously-hidden Task fields on cards with deliberate hierarchy: priority (P0/P1 lead title, semantic colors; P2/P3 quiet), assignment (taken_by_role + lane.branch icon-led), peer, group — render only when present, W2 band. Fixed title-collapse-to-1-glyph bug. 0 overflow W1-W3 on synthetic + real plan; tsc + cloudFlush green; perf layout untouched. FIELD-COVERAGE BACKLOG for next iterations: (1) plan/spec heartbeat(current_focus/active_lanes)+loops status 0% surfaced; (2) milestone author(from)+loop_id; (3) task lane.* detail @W3; (4) handoff/decision structured content unparsed; (5) orchestrator-group counts+heartbeat; (6) repo-wide bare-string->enum sweep. SEPARATE BUGS flagged: daemon not indexing .codeterm/plans/*.json (4 real files, index=0); pre-existing worlds.test.ts crispRasterScale failure (perf lane).
