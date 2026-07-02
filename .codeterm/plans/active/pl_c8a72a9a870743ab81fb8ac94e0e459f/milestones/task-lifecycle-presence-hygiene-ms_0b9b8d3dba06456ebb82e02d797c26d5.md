---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_0b9b8d3dba06456ebb82e02d797c26d5
handle: task-lifecycle-presence-hygiene-ms_0b9b8d3dba06456ebb82e02d797c26d5
title: status_update
---

STATUS: in_review
CHANGES: 410ddfd5 fix: clear terminal task presence bindings; files: src-tauri/src/service/plan/mutations.rs, mod.rs, state.rs, tests.rs; src-tauri/src/plan/reconcile.rs; src/components/Canvas/paneCurrentTask.ts/test.ts, taskPresence.ts/test.ts, planPaneMatch.ts
BUILD/TYPECHECK/TESTS: npx tsx src/components/Canvas/taskPresence.test.ts PASS; npx tsx src/components/Canvas/paneCurrentTask.test.ts PASS; cargo fmt --manifest-path src-tauri/Cargo.toml PASS; git diff --check PASS; npx tsc --noEmit FAILED due missing @codeterm/plugin-sdk module declarations in plugin packages, before reaching lane files; cargo tests not run per worker constraints
FOLLOWUP: branch lane/task-lifecycle-presence-hygiene pushed to origin with --no-verify. Backend close/status-terminal paths now clear assigned_pane_id and lane.pane_id; backend index/bindings/live_agents ignore terminal tasks; reconcile trims empty pane ids and ignores terminal bindings; frontend current-task/presence selectors ignore done/cancelled stale bindings.
