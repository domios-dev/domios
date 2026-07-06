---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_980730f637c64bcdae55a29800555450
handle: remote-qa-builders-git-backed-gate-execution-across-registered-mesh-peer-ms_980730f637c64bcdae55a29800555450
title: review
---

Bounced internal QA runner at 57f60d0a. Diff is correctly internal scripts/docs only, but acceptance gate failed in real orchestrator env: scripts/qa-gate.sh run shell-guards --allow-dirty --timeout 120 produced QA_RUN status=fail because inherited prod CODETERM_API_PORT=7685 leaked into check-build-kill-safety. Required fix: sanitize dev-safe env for gates while preserving explicit overrides.
