---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_1761b25a541b461ab87694621efe09d0
handle: add-cli-api-respawn-from-task-history-ms_1761b25a541b461ab87694621efe09d0
title: status_update
---

PARTIAL: STATUS: partial
CHANGES: Added task execution attempt history model, CLI/RPC/REST list-show-respawn surfaces, respawn prompt construction with task body/last report/lane/model/report command, and attempt updates on assignment/report. Regenerated docs/CLI manifest and updated docs/API. Split oversized touched tests into focused modules.
BUILD/TYPECHECK/TESTS: cargo test --manifest-path cli/Cargo.toml plan_task_attempts_and_respawn_parse: pass; cargo test --manifest-path cli/Cargo.toml plugin_subcommands_parse: pass; cargo test --manifest-path cli/Cargo.toml cmd::manifest: pass; cargo test --manifest-path src-tauri/Cargo.toml task_attempt_history_builds_respawn_prompt_from_prior_attempt: pass using temporary dist/index.html plus existing bundle stub helpers for Tauri build-script resources; git diff --check: pass; bash scripts/check-file-sizes.sh still reports pre-existing oversized files outside this lane (provider-codex plugin.ts, js_runtime/provider.rs, keepawake.rs) while touched oversized test files are now under 1000 lines.
FOLLOWUP: in_review report was refused by report_guard because the lane has uncommitted changes and I cannot commit without explicit user request. Review whether Cargo.lock version refresh from 1.6.0 to 1.6.1 should be kept; it matches current crate manifests and was produced by cargo.
