---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_c481715d091045e28cc09f8b81a9c062
handle: ui-ux-iter10-task-cards-show-executing-agents-reusable-agentcircle-stack
title: UI/UX iter10 — task cards show executing agents (reusable AgentCircle/Stack)
status: done
---

presence/identity.ts (PresenceAgent + presenceColor 10-hue deterministic palette + agentInitials); presence/AgentCircle.tsx (reusable AgentCircle + AgentStack: overlapping avatars, +N overflow, live pulse, position-agnostic for iters 11-12); taskPresence agents[]+branch (lead label pane-title->role->short-id); TaskCard presence cluster in always-visible HEADER (color stack+lead+branch+activity, wraps). perfFixture synthetic presence (flagged). Live wiring (pane/agent registry->plan) = next data dep. 0 overflow W0-W3; tsc+cards 20/20+taskPresence+cloudFlush 9/9; perf untouched. Committed feat/canvas-perf-polish (unpushed).
