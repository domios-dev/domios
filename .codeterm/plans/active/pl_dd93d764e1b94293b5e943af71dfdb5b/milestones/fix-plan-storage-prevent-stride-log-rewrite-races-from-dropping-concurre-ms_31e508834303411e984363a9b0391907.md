---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_31e508834303411e984363a9b0391907
handle: fix-plan-storage-prevent-stride-log-rewrite-races-from-dropping-concurre-ms_31e508834303411e984363a9b0391907
title: status_update
---

Accepted and merged through 914dcf39. Ordinary save now preserves external stride appends after stale snapshots instead of replacing concurrent sidecar entries. Gates: ordinary_save_preserves_external_stride_append_after_stale_snapshot included in service::plan::body_file_tests 11/11 PASS under scripts/test-capped.sh; cargo fmt --check PASS; product git diff --check PASS; file-size gate PASS; pre-push cargo check/fmt/clippy PASS.
