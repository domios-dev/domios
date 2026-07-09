---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_0500d133311a4e4189cf6aa7d221dea3
handle: ui-ux-iter16-orchestrator-progress-strip-above-the-composer
title: UI/UX iter16 — orchestrator progress strip above the composer
status: done
---

OrchestratorProgressStrip: workflow glyph + plan title + determinate meter (bar + pct% · done/total) + active-lanes chip + current_focus narrative + swarm face-stack + +N plans tail + fly-to glyph; heartbeat-green pulse when live. paneOwnedPlans.ts resolver (owner IS this pane, not ancestors; summarize done/total/pct/lanes/live/focus/agents, most-active-first). PaneTaskStrip variant gate: owns plan->progress (flies to plan overview) / has task->worker strip / else null. Responsive: focus yields width before title (readable at 460px). 0 overflow 760/900/460/100%; tsc+cloudFlush 9/9+cards 20/20+strip 6/6; CanvasView 729; all <1000. Committed feat/canvas-perf-polish (unpushed).
