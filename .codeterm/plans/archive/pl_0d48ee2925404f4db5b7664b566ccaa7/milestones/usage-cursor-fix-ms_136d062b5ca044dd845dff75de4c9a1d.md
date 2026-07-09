---
plan_id: pl_0d48ee2925404f4db5b7664b566ccaa7
kind: milestone
id: ms_136d062b5ca044dd845dff75de4c9a1d
handle: usage-cursor-fix-ms_136d062b5ca044dd845dff75de4c9a1d
title: testing
---

RUNTIME bug found+fixed. Cursor zero-snapshots root cause: poller's visible-pane gate skipped fetch-based cursor (no terminal to gate on) — only manual refresh worked. Fix: usage_is_fetch_based() capability + poller polls fetch providers without a pane. VERIFIED LIVE on 7695: no cursor pane open, poller auto-fetched api2.cursor.sh, stored spend snapshot; cursor bubble renders $19.17 + 'spent this cycle resets Jul 1' popover
