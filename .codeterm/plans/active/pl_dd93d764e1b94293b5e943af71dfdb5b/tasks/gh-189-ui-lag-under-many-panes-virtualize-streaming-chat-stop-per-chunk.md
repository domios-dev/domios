---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_2ff37f584b274f34a3277c20edb0f56a
handle: gh-189-ui-lag-under-many-panes-virtualize-streaming-chat-stop-per-chunk
title: 'GH #189: UI lag under many panes — virtualize streaming chat, stop per-chunk reflow'
status: done
---

STEP 1: gh issue view 189 — issue body is the spec (non-virtualized streaming chat + per-chunk reflow/scroll + composited-layer bloat). RELATED EXISTING LANES — read their strides first, do not duplicate: task_8b5354971e124bb5885cb36b3472f17d (pane-mount xterm longtasks), task_85cec85bfdae43c283558ddcdd1d13c7 (view-switch longtasks), loop_f5854596 (tab-switch <150ms oracle). THIS task owns the chat-view-specific part: (1) virtualize the message list (only visible messages in DOM; 1000-message transcript must not mean 1000 mounted bubbles); (2) batch streaming chunks (rAF-coalesced appends, no per-chunk layout/scroll); (3) audit composited layers (will-change/transform bloat across panes). Measure with the existing [LONGTASK]/[SLOW-IPC] dev instrumentation under 10+ panes with 2 streaming agents.
