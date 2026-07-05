---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8b5354971e124bb5885cb36b3472f17d
handle: terminal-reduce-pane-mount-xterm-longtasks-from-responsiveness-soak
title: 'Terminal: reduce pane-mount xterm longtasks from responsiveness soak'
status: done
---

Investigate and reduce frontend main-thread longtasks caused by agent pane mount / xterm initialization during parallel spawns. Evidence from responsiveness soak on eight: six rAF gaps >250ms, max 517ms; gaps correlate with browser longtasks during agent pane mount/xterm init, with zero backend SLOW-IPC/LOCK-HOLD. Keep backend untouched unless a measured frontend-to-backend boundary proves otherwise. Start from TerminalPane/xterm initialization and tab mount/render paths. Deliver a narrow diff plus before/after soak numbers or a focused reproduction showing reduced max gap.
