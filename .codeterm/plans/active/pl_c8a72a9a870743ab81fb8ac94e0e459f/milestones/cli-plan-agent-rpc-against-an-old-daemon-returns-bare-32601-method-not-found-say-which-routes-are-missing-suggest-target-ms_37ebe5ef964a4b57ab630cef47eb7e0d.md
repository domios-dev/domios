---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_37ebe5ef964a4b57ab630cef47eb7e0d
handle: cli-plan-agent-rpc-against-an-old-daemon-returns-bare-32601-method-not-found-say-which-routes-are-missing-suggest-target-ms_37ebe5ef964a4b57ab630cef47eb7e0d
title: testing
---

ACCEPTED. -32601 detection in cli/src/cmd/plan_errors.rs (new) + client.rs emits helpful hint (target port + --instance dev/prod/CODETERM_API_PORT guidance); cli/src/cmd/task.rs wired. cli crate cargo check CLEAN (3.74s). Boundaries respected (not plan.rs/spec.rs/hints.rs). @4a2599e4 on canvas/cli-rpc-hint. Friction noted: build hash unavailable in RPC error obj -> hint shows port only (acceptable).
