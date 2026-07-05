---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_570f7340da91481a9620ed67103ef58e
handle: codex-usage-status-probe-reliability-ms_570f7340da91481a9620ed67103ef58e
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: src-tauri/src/usage/probe.rs; commit 3fe0e504 pushed to origin/lane/usage-codex-status
BUILD/TYPECHECK/TESTS: cargo fmt on probe.rs passed. Rust tests/build not run per worker constraint; orchestrator should run usage::probe tests, provider-codex parser tests, build.sh check, and live codeterm usage refresh --provider codex smoke.
FOLLOWUP: Probe now captures output offset + screen hash before each slash command, ignores unchanged stale live screens and old scrollback, treats /status first command as a priming epoch, then accepts only the second/third command epoch.
