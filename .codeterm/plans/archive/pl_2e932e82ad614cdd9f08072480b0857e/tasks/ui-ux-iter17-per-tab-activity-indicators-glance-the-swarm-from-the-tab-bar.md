---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_a500973448524356b125c115e2d7e35d
handle: ui-ux-iter17-per-tab-activity-indicators-glance-the-swarm-from-the-tab-bar
title: UI/UX iter17 — per-tab activity indicators (glance the swarm from the tab bar)
status: done
---

Each session tab wears a tiny plan-membership glance: worker tab -> board-identity dot (presenceColor(paneId), same hue as canvas AgentCircle); orchestrator tab -> live-green Workflow glyph; live pane pulses; no plan -> none (honest). Hover: worker=plan title(+N); orch=plan title/N tasks. shrink-0 never clips title. Factored ownerIsPaneSelf into planPaneMatch.ts (single owner-self rule for strip+tab). paneTabActivity.ts INDEX-ONLY (cheap gate, no full-body pull, runs every tab ~zero cost); Sidebar passes one value-compared activity prop (memo holds, one mux subscriber). 0 overflow; tsc+TabActivityIndicator 9/9+cloudFlush 9/9+strip 6/6+WorkerTaskStrip 10/10+multiview 2/2; CanvasView 729; all <1000 (Sidebar 977). Committed feat/canvas-perf-polish (unpushed).
