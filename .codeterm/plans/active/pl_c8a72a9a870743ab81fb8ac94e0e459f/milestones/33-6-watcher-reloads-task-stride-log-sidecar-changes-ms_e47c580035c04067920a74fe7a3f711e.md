---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_e47c580035c04067920a74fe7a3f711e
handle: 33-6-watcher-reloads-task-stride-log-sidecar-changes-ms_e47c580035c04067920a74fe7a3f711e
title: status_update
---

Accepted and integrated into feat/canvas.

Changes:
- Plan watcher now maps `.codeterm/plans/<plan_id>/tasks/<task-handle>/strides.jsonl` changes back to `<plan_id>`.
- Project-root resolution now handles that JSONL sidecar shape.
- Existing JSON and section/task markdown path behavior remains covered.

Orchestrator verification in /Users/rollacode/Developer/codeterm:
- cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::watcher -- --nocapture (2 passed)
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::body_file_tests::task_stride_log_hydrates_when_inline_milestones_are_missing -- --nocapture (1 passed)
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::stride_log -- --nocapture (7 passed)
- git diff --check for touched file
- bash scripts/check-file-sizes.sh (only known unrelated baseline files; touched file under limits)
- CODETERM_API_PORT=7686 ./build.sh check

Residual:
- This is path classification + reload trigger plumbing. Full live notify debounce/end-to-end merge simulation remains broader than this slice.
