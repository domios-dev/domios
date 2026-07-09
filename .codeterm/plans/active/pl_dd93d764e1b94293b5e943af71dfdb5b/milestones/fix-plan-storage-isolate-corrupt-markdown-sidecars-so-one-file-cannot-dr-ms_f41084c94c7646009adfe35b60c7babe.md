---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f41084c94c7646009adfe35b60c7babe
handle: fix-plan-storage-isolate-corrupt-markdown-sidecars-so-one-file-cannot-dr-ms_f41084c94c7646009adfe35b60c7babe
title: status_update
---

Accepted and merged through 914dcf39. Corrupt markdown sidecar read errors now warn and preserve JSON/inline body so scan/reload keeps the plan indexed. Gates: service::plan::body_file_tests 11/11 PASS under scripts/test-capped.sh; cargo fmt --check PASS; product git diff --check PASS; file-size gate PASS; pre-push cargo check/fmt/clippy PASS.
