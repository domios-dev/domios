---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5cc2ea930b89424c80d5d9f7a9ff672e
handle: externalize-bodies-to-md-files-ms_5cc2ea930b89424c80d5d9f7a9ff672e
title: status_update
---

Reconciled as already implemented. Evidence: task milestone from Main says section/task bodies were externalized into sibling markdown files with hydrate/migrate and watcher reload. Source verification: src-tauri/src/service/plan/body_files.rs persists/strips/hydrates sections and tasks, writes INDEX.md, removes stale id paths; PlanStore save/get paths call persist_bodies/strip_bodies/hydrate_and_migrate; plan files CLI renderer exists. Gate run now: perl -e 'alarm 90; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml service::plan::body_file_tests -- --nocapture (section_and_task_bodies_roundtrip_through_markdown_files passed). No worker spawned because this was stale plan state, not missing implementation.
