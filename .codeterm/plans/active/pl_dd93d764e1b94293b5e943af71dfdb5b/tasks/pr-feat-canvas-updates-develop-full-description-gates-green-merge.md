---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_b3789e40f7884dc6ac806a80a064c0cb
handle: pr-feat-canvas-updates-develop-full-description-gates-green-merge
title: 'PR feat/canvas-updates -> develop: full description, gates green, merge'
status: pending
---

GATED ON: all five review lanes closed, all review-fix tasks closed, both regression lanes (ROG + ubunta) closed green or with accepted+documented exceptions. Steps: (1) integration checkout clean (no WIP, no stray .codeterm sidecar noise in the diff — plan sidecars under .codeterm/plans are part of the repo state, include them consciously); (2) full local gate: cargo check, fmt, clippy both crates, scripts/check-file-sizes.sh, tsc, focused test sanity (the full suites just ran on two machines — do not re-run everything locally, reference those results); (3) write the PR description covering the WHOLE branch (758 commits): Board/List/Canvas redesign, planner/orchestrator peer model + subordination, pair spawn + wait-for-goal gates, sidebar containment + taxonomy, mission control, mobile pass, comment hygiene sweep (-3022 LOC), prompt architecture, build-speed infra (sccache/profiles/tiered pre-push), test discipline (capped runner, Playwright rules), regression evidence links (ROG + ubunta run strides); reference issues with bare #N (NEVER 'Closes #N' — close-on-publish is the pipeline rule); (4) open PR to develop, wait for CI green, merge. Record PR URL + merge sha as strides.
