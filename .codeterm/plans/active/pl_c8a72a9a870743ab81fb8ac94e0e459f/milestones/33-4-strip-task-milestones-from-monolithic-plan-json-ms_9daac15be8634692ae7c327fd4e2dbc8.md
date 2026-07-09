---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_9daac15be8634692ae7c327fd4e2dbc8
handle: 33-4-strip-task-milestones-from-monolithic-plan-json-ms_9daac15be8634692ae7c327fd4e2dbc8
title: status_update
---

Accepted and integrated into feat/canvas.

Changes:
- Saved plan JSON now strips task milestone arrays after stride sidecar persistence.
- Legacy inline-only milestones still load, then save into tasks/<handle>/strides.jsonl and disappear from raw plan JSON.
- Existing sidecar hydration and markdown body behavior remains covered.

Orchestrator verification in /Users/rollacode/Developer/codeterm:
- cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::body_file_tests -- --nocapture (7 passed)
- perl -e 'alarm 180; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib service::plan::stride_log -- --nocapture (7 passed)
- git diff --cached --check && git diff --check
- bash scripts/check-file-sizes.sh (only known unrelated baseline files; touched files under limits)
- CODETERM_API_PORT=7686 ./build.sh check
