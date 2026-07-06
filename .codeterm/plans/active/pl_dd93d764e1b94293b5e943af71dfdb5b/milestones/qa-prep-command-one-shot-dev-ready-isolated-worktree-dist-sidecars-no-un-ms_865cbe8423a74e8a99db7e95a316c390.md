---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_865cbe8423a74e8a99db7e95a316c390
handle: qa-prep-command-one-shot-dev-ready-isolated-worktree-dist-sidecars-no-un-ms_865cbe8423a74e8a99db7e95a316c390
title: status_update
---

STATUS: in_review
CHANGES: build.sh line 138 — prod-port guard now exempts qa-prep (CMD != qa-prep) | SHA: 485a6137 | branch: lane/460103461-qa-prep
VERIFIED: bash -n syntax ok (did NOT build)
FIX: Changed 'if [ "$CT_PORT" = "7685" ]' to 'if [ "$CT_PORT" = "7685" ] && [ "$CMD" != "qa-prep" ]' at the hard exit guard. qa-prep uses no daemon/port so the guard is irrelevant; an orchestrator in a prod pane (CODETERM_API_PORT=7685 inherited) can now run ./build.sh qa-prep without needing a port override.
NOTE: build|cli|check|test|clean|env are also daemon-less but not changed — keeping scope to qa-prep as instructed. If those should be exempted too, that's a separate PR.
