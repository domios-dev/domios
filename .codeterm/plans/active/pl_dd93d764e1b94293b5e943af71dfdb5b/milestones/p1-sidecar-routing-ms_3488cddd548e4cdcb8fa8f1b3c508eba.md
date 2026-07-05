---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3488cddd548e4cdcb8fa8f1b3c508eba
handle: p1-sidecar-routing-ms_3488cddd548e4cdcb8fa8f1b3c508eba
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: 31e75ab5 fix: align canonical sidecar index cleanup; delete_plan_units now uses the canonical plan root; reindex_project_index filters plans/specs by canonical roots; stale/deleted canonical_project_path now normalizes back to the loaded project root; focused tests updated for delete/reindex and stale canonical fallback.
BUILD/TYPECHECK/TESTS: cargo fmt in src-tauri passed; git diff --check passed; CARGO_TARGET_DIR=/Users/rollacode/Developer/codeterm/src-tauri/target cargo test --manifest-path cli/Cargo.toml storage_warns_about_stray_linked_worktree_sidecar_copy passed. Backend focused cargo test was not rerun after this patch because the prior run failed before tests in Tauri build.rs due missing bundled-embed/codeterm-embed-aarch64-apple-darwin; orchestrator should run the warm backend gate.
FOLLOWUP: No P1b mesh homing added. Branch lane/p1-sidecar-routing pushed with --no-verify and worktree is clean.
