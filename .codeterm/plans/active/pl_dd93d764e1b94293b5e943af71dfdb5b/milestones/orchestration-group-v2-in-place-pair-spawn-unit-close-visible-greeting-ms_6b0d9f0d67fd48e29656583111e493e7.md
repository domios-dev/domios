---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_6b0d9f0d67fd48e29656583111e493e7
handle: orchestration-group-v2-in-place-pair-spawn-unit-close-visible-greeting-ms_6b0d9f0d67fd48e29656583111e493e7
title: decision
---

ROUND-18 HARD REQUIREMENT (user, architectural — no patches): the empty New-session tab must be VIRTUAL. (1) NO PTY/shell/session is spawned when the tab opens — it is a pure GUI surface until the user confirms a choice; the pane/PTY comes into existence only at confirm. (2) NOT persisted: the picker tab is excluded from session snapshots — after daemon/app restart it simply does not reappear (no restored empty husks). (3) Closing it costs nothing and leaks nothing. Acceptance addition: pane list / PTY count unchanged while the picker is open (assert in the matrix); session snapshot taken with an open picker contains no entry for it.
