---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b2a5cdd5d6104354879dad796fe31ec3
handle: bug-concurrent-plan-task-add-returns-rpc-32603-database-is-locked-but-the-write-still-commits-misleading-error-ms_b2a5cdd5d6104354879dad796fe31ec3
title: status_update
---

Integrated the plan search-index DB-lock retry lane into the hot checkout.

Verified by Orchestrator:
- cargo fmt --manifest-path src-tauri/Cargo.toml: passed
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::search_index::tests::add_task_survives_transient_plan_index_write_lock -- --nocapture: passed
- CODETERM_API_PORT=7686 ./build.sh check: passed
- git diff --check: passed
- restarted dev on 7686 and ran live concurrent smoke: created temporary plan pl_450e1d4e1f1a47729faccd6d31da4abb, fired 8 parallel `plan task add`; all 8 exited 0 and task list returned count=8.

Notes:
- Fix retries transient busy/locked errors around plan index replacement writes and keeps the plan task write visible/searchable.
- Smoke left temporary dev-only plans titled db-lock-smoke-* in the dev plan store.
