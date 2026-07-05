---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_7289449d75f74b70872337112b5197d7
handle: add-cli-api-respawn-from-task-history-ms_7289449d75f74b70872337112b5197d7
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: Amended c3b08f1c to use canonical Task.execution_attempts/TaskExecutionAttempt for task history; CLI/API attempts/list/show/respawn now select by executor_pane_id-derived attempt_id, derive prompt/report_command at response time, and append a new canonical execution_attempt for respawns. Report body is stored narrowly on TaskExecutionReportSnapshot.message so respawn prompts include prior report text/status. No Task.attempts duplicate schema remains.
BUILD/TYPECHECK/TESTS: git status --short clean; git diff --check passed; cargo fmt for cli and src-tauri; cargo test --manifest-path cli/Cargo.toml plan_task_attempts_and_respawn_parse passed; cargo test --manifest-path cli/Cargo.toml cmd::manifest passed; node scripts/prepare-cli-stub.js && node scripts/prepare-embed-stub.js && mkdir -p dist && touch dist/index.html && cargo test --manifest-path src-tauri/Cargo.toml task_attempt_history_builds_respawn_prompt_from_prior_attempt passed, dist removed after run.
FOLLOWUP: Backend merge should preserve TaskExecutionReportSnapshot.message or another canonical report-body slot; no Cargo.lock noise included.
