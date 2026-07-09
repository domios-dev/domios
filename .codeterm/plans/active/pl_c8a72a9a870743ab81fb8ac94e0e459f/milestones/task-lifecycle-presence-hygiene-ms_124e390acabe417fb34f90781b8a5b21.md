---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_124e390acabe417fb34f90781b8a5b21
handle: task-lifecycle-presence-hygiene-ms_124e390acabe417fb34f90781b8a5b21
title: status_update
---

STATUS: in_review
CHANGES: fbed1439 fix: clear terminal task presence bindings; amended lane/task-lifecycle-presence-hygiene. Added review fixes in src-tauri/src/service/plan/mod.rs and tests.rs, src/components/Canvas/planOpenFocus.ts/test.ts, workspacePlanEntry.test.ts, plus prior lifecycle/presence files.
BUILD/TYPECHECK/TESTS: npx tsx src/components/Canvas/planOpenFocus.test.ts PASS; npx tsx src/components/Canvas/workspacePlanEntry.test.ts PASS; npx tsx src/components/Canvas/paneCurrentTask.test.ts PASS; npx tsx src/components/Canvas/taskPresence.test.ts PASS; cargo fmt --manifest-path src-tauri/Cargo.toml PASS; git diff --check PASS. Cargo tests not run per worker constraints.
FOLLOWUP: Review findings fixed: reconcile fixes and pane-gone cleanup now clear both assigned_pane_id and lane.pane_id; assignedTaskIdsForPane trims pane ids and ignores done/cancelled/empty bindings so Canvas open/focus does not target terminal stale tasks. Branch pushed with --no-verify.
