---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b7e15ba30c174f74977df5854ad64a8c
handle: mobile-collapsible-compact-plan-header-phone-viewports-ms_b7e15ba30c174f74977df5854ad64a8c
title: decision
---

SCOPE ADDITION (round-32c, user): mobile tap targets — sidebar rows/tabs are too narrow on phone, hard to hit. On coarse-pointer/narrow viewports every interactive row (tabs, plan entry, section headers, expand chevrons, badges acting as buttons) gets >=44px hit height (Apple HIG minimum) — visual density may stay via inner padding, but the HIT AREA must be 44px. Add to the Playwright scenarios: bounding-box height assertion >=44 for tab rows and the compact-header bar at 390x844.
