---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6b8a7b7252fd484ab6e4e3e4e0288515
handle: add-cli-api-respawn-from-task-history-ms_6b8a7b7252fd484ab6e4e3e4e0288515
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: Committed 52ac23c9 (feat: respawn workers from task history). Added append-only task execution attempts, CLI/socket/REST attempts list-show-respawn surfaces, respawn prompt construction with task body/last report/lane/model/exact report command, and attempt updates on assignment/report. Updated generated CLI docs/manifest and API docs. Split touched oversized tests into focused modules; lockfile version-only noise restored and excluded.
BUILD/TYPECHECK/TESTS: git status --short clean; git diff --check pass; cargo fmt for cli and src-tauri run before commit; post-commit tests passed: cargo test --manifest-path cli/Cargo.toml plan_task_attempts_and_respawn_parse; cargo test --manifest-path cli/Cargo.toml plugin_subcommands_parse; cargo test --manifest-path cli/Cargo.toml cmd::manifest; cargo test --manifest-path src-tauri/Cargo.toml task_attempt_history_builds_respawn_prompt_from_prior_attempt (with temporary dist/index.html plus bundle stub helpers for Tauri build-script resources, removed afterward).
FOLLOWUP: Residual risk: full src-tauri test suite/build not run per lane constraint; file-size script still reports pre-existing unrelated oversized files outside this lane (provider-codex plugin.ts, js_runtime/provider.rs, keepawake.rs).
