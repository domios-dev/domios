---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_4d16263b14b34bf8823735ea4e46e66c
handle: 33-5-preserve-merged-stride-log-entries-on-save-ms_4d16263b14b34bf8823735ea4e46e66c
title: status_update
---

Accepted and integrated into feat/canvas.

Changes:
- `persist_stride_log` now reads existing `tasks/<handle>/strides.jsonl`, combines it with the task's current milestones, folds/dedupes deterministically, and rewrites the folded log.
- This preserves sidecar-only entries from git union merges or manual sidecar edits during ordinary plan saves.
- Raw plan JSON remains stripped of `tasks[].milestones`.

Orchestrator verification in /Users/rollacode/Developer/codeterm:
- cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::body_file_tests::save_preserves_existing_sidecar_only_stride_entries -- --nocapture (1 passed)
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::body_file_tests -- --nocapture (8 passed)
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::stride_log -- --nocapture (7 passed)
- git diff --check for touched files
- bash scripts/check-file-sizes.sh (only known unrelated baseline files; touched files under limits)
- CODETERM_API_PORT=7686 ./build.sh check
