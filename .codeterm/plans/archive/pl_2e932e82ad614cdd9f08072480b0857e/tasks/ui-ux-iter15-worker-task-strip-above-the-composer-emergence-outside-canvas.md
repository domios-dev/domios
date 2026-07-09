---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_95ade7c944fd4ea7b871d9799807d17f
handle: ui-ux-iter15-worker-task-strip-above-the-composer-emergence-outside-canvas
title: UI/UX iter15 — worker task strip above the composer (emergence outside canvas)
status: done
---

WorkerTaskStrip above agent pane input: agent dot (identity color, live-pulse in_progress) + handle + ellipsized name + status pill + branch chip + agent left-rail + fly-to glyph; task=null renders nothing. paneCurrentTask.ts resolver (assigned_pane_id/lane.pane_id across active plans, liveness-ranked). PaneTaskStrip cheap index-gate (only task-bearing panes pull plan bodies, null early otherwise). Click -> openPanePlanFocus -> opens canvas + flies to task. ChatComposer +2 lines. 0 overflow 900/460px; tsc+cloudFlush 9/9+paneCurrentTask 8/8+WorkerTaskStrip 10/10+Playwright real-wiring pass; canvas perf untouched; all files <1000. Committed feat/canvas-perf-polish (unpushed).
