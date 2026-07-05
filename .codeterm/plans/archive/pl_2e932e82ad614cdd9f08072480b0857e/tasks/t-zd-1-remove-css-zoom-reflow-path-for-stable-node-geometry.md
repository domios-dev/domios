---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_7a10164bf5994ebca2f892c6eeaaead6
handle: t-zd-1-remove-css-zoom-reflow-path-for-stable-node-geometry
title: T-ZD.1 — Remove CSS zoom reflow path for stable node geometry
status: done
tags:
- canvas
- zoom
- implementation
- substrate
---

Current T-ZD.1 status:
- Remote Opus re-tasked Codex worker b47a821c and it is working on the isolated Playwright path.
- Worker is expected to run isolated CODETERM_INSTANCE=zd1 on port 7697, author tests/playwright/suites/13-canvas/canvas-zoom-drift.spec.ts, and run slot-rect linearity plus 13-canvas worlds goldens against lane code.
- Shared dev 7686 was restored after a failed start and is healthy again. It should not be used for this lane's Playwright gate.
- Local build.sh safety patch now also cleans stale web watchers from the current worktree to avoid future dev-console clutter.
