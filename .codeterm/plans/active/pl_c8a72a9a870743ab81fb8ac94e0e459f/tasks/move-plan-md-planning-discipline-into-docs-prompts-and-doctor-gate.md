---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5c7f695b6fd744a7a6750fb96c29da2f
handle: move-plan-md-planning-discipline-into-docs-prompts-and-doctor-gate
title: Move plan.md planning discipline into docs, prompts, and doctor gate
status: done
---

Move plan.md guidance into product docs/prompts and CLI quality gates.

Scope:
- Extend docs/ORCHESTRATION with CodeTerm planner source-of-truth, Goal/Spec/Plan/Task/Stride order, Superpowers-as-quality-checklist, and approval gate.
- Update embedded prompts so raw agents learn they can become orchestrators and must read docs ORCHESTRATION when promoted.
- Add plan doctor --kind quality checks for goal/spec/file ownership/approval/verification/task quality/planning misuse.
- Regenerate docs/CLI.md and docs/cli-manifest.json from clap.
- Keep mem preference hints valid; use existing mem save syntax until a dedicated shortcut lands.

Acceptance:
- docs ORCHESTRATION explains doctor -> approve -> task generation.
- CLI manifest/docs freshness tests pass.
- Prompt tests pass.
- File-size gate passes.

Gates already run locally:
- cargo test --manifest-path cli/Cargo.toml cmd::manifest -- --nocapture PASS
- cargo test --manifest-path cli/Cargo.toml plan_doctor -- --nocapture PASS
- cargo test --manifest-path src-tauri/Cargo.toml prompts -- --nocapture PASS
- bash scripts/check-file-sizes.sh --fail PASS
