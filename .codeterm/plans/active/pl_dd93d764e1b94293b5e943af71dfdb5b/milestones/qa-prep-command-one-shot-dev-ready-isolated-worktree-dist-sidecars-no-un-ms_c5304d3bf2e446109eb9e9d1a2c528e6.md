---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_c5304d3bf2e446109eb9e9d1a2c528e6
handle: qa-prep-command-one-shot-dev-ready-isolated-worktree-dist-sidecars-no-un-ms_c5304d3bf2e446109eb9e9d1a2c528e6
title: review
---

GATE FINDING — qa-prep is blocked by build.sh's hard prod-port guard (line 138: CT_PORT=7685 -> exit 1). qa-prep never targets a daemon/port, yet an orchestrator running inside a PROD pane (its exact intended user) inherits CODETERM_API_PORT=7685 and can't run it. Real usability bug: qa-prep must be exempt from the port guard (it's pure build-prep like build/cli/check). Verifying the qa-prep dist+sidecar logic now via a port override; will bounce the guard-exemption fix to the worker.
