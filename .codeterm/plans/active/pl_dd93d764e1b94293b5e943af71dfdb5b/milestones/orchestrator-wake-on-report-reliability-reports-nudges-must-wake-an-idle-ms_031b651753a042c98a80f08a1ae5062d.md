---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_031b651753a042c98a80f08a1ae5062d
handle: orchestrator-wake-on-report-reliability-reports-nudges-must-wake-an-idle-ms_031b651753a042c98a80f08a1ae5062d
title: status_update
---

Accepted and merged wake-on-report reliability in bd0a041e. Reviewed the dropped-newest-needle bounce fix: per-PTY watcher now tracks the latest body, stale drains do not retire the watcher, and regression tests cover bursty second delivery/no double-submit. Local gates passed: service::pty_io::wake 7/7, service::messenger 23/23, service::inject_verify 20/20, comment-block gate, file-size gate, diff-check excluding .codeterm, pre-push cargo check/fmt/clippy. Cross-machine focused wake test requested on ROG and ubunta for additional evidence.
