---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_6c70a2d0c9904fad851a04fffb82e1ed
handle: wrap-orchestrator-non-plan-rpc-errors-ms_6c70a2d0c9904fad851a04fffb82e1ed
title: status_update
---

Accepted. Orchestrator non-plan RPC bypasses now render workflow-specific recovery messages for agent.reports status and handoff reseed pane lookup/session failures. Verified by orchestrator: diff only cli/src/cmd/orchestrator.rs, cargo test --manifest-path cli/Cargo.toml orchestrator -- --nocapture 10/10, git diff --check.
