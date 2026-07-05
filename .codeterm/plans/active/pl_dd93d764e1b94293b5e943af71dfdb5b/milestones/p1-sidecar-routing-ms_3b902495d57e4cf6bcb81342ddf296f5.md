---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3b902495d57e4cf6bcb81342ddf296f5
handle: p1-sidecar-routing-ms_3b902495d57e4cf6bcb81342ddf296f5
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: 47a853ca fix: route plan sidecars to canonical project; touched plan/spec model path fields, sidecar path routing, CLI doctor/project-root handling, plan get/state rendering, and focused tests.
BUILD/TYPECHECK/TESTS: cargo fmt in src-tauri and cli passed; git diff --check passed; focused cargo test create_from_linked_worktree_persists_canonical_project_path_and_routes_sidecars attempted with CARGO_TARGET_DIR=/Users/rollacode/Developer/codeterm/src-tauri/target but failed before tests in Tauri build script because bundled-embed/codeterm-embed-aarch64-apple-darwin is missing. No broad build/test run per worker constraints.
FOLLOWUP: Orchestrator should run the warm build/check gate and live smoke. Doctor now warns stray_sidecar_copy for non-canonical linked-worktree copies and names the canonical project path.
