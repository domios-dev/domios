---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_b085585bf935414288955e01ff625517
handle: branch-review-r1-rust-plan-service-storage-diff-vs-develop-ms_b085585bf935414288955e01ff625517
title: review
---

HIGH: src-tauri/src/service/plan/body_files.rs:567 and src-tauri/src/service/plan/body_files.rs:599 still let one corrupt markdown sidecar kill the entire plan load. Current behavior: hydrate_plain_markdown/hydrate_body/hydrate_element_markdown call std::fs::read_to_string(path)?; hydrate_and_migrate propagates that io::Error to PlanStore::load_file_into_index at src-tauri/src/service/plan/mod.rs:361, and scan_project logs 'skip malformed plan' at src-tauri/src/service/plan/mod.rs:414-416. A single invalid-UTF8/torn goal.md, task .md, milestone .md, or element .md therefore removes the whole plan from the in-memory index even though plan.json and other sidecars are usable. Concrete fix: make sidecar hydration isolate per-file failures: warn, preserve inline/JSON value or leave existing field unchanged, record repair debt if needed, and continue loading the plan. Add tests for invalid UTF-8 in a task or milestone markdown sidecar and assert scan_project/reload keeps the plan and surviving strides/tasks.
