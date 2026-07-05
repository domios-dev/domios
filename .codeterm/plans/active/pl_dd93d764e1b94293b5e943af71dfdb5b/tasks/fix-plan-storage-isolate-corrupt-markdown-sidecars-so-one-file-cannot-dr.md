---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_321a3a84ac404b0b83643db41d2069b9
handle: fix-plan-storage-isolate-corrupt-markdown-sidecars-so-one-file-cannot-dr
title: 'fix(plan-storage): isolate corrupt markdown sidecars so one file cannot drop a plan'
status: done
---

Review R1 found a HIGH resilience bug. Current behavior: src-tauri/src/service/plan/body_files.rs:567 hydrate_plain_markdown, src-tauri/src/service/plan/body_files.rs:582 hydrate_element_markdown, and src-tauri/src/service/plan/body_files.rs:599 hydrate_body call std::fs::read_to_string(path)? on user-editable sidecar markdown. Any invalid UTF-8 or read error propagates through hydrate_and_migrate at src-tauri/src/service/plan/body_files.rs:286 to PlanStore::load_file_into_index at src-tauri/src/service/plan/mod.rs:361-362, and scan_project then skips the entire plan at src-tauri/src/service/plan/mod.rs:414-416. Desired behavior: sidecar hydration should catch per-file errors, warn with path/context, keep the inline/JSON value or leave that field unchanged, and continue loading the plan. If a repair surface exists, record repair debt for the corrupt sidecar. Test: corrupt one task/milestone markdown sidecar with invalid UTF-8 while plan.json and strides.jsonl are valid; scan/reload must keep the plan indexed and preserve surviving records.
