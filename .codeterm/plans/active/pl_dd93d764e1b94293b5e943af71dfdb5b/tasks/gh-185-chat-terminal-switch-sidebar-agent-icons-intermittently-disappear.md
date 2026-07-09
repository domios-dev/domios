---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_cfc7151eff3344bb9bf00f27571c6463
handle: gh-185-chat-terminal-switch-sidebar-agent-icons-intermittently-disappear
title: 'GH #185: Chat|Terminal switch + sidebar agent icons intermittently disappear (verify-first)'
status: done
---

STEP 1: gh issue view 185 — issue body is the spec. Intermittent: the Chat|Terminal face switch and sidebar agent icons disappear. VERIFY-FIRST: the sidebar and tab-row code was heavily reworked on this branch (containment, taxonomy, provider glyphs) — this may already be fixed. Reproduce on current dev: cycle 20+ chat/terminal switches across agent panes, restart dev mid-session, watch for icon loss (the R5 review lane also swept this area — read its findings on unstable keys/remount storms first). If reproduced: root cause + fix; if not reproducible after an honest attempt (document the attempts), close #185 with the evidence.
