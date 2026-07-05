---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6c4bb9a9e85742568da54bf7bfa5b8cc
handle: render-structured-planner-cli-errors-ms_6c4bb9a9e85742568da54bf7bfa5b8cc
title: status_update
---

Accepted. Structured planner CLI errors now render actionable messages for bad_mode, bad_priority, context-aware not_found, plan-vs-task bad_status, and io/handoff cases; added guard coverage that current backend PlanError types do not fall back to raw RPC. Verified by orchestrator: cargo test --manifest-path cli/Cargo.toml cmd::plan::tests -- --nocapture 25/25, orchestrator tests 10/10, git diff --check.
