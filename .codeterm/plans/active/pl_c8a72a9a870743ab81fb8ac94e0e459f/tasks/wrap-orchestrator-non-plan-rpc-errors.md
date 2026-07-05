---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_94522a3bf01f48369de971ea09ba8c9e
handle: wrap-orchestrator-non-plan-rpc-errors
title: Wrap orchestrator non-plan RPC errors
status: done
---

Fix raw non-plan RPC bypasses in orchestrator CLI found by audit. Scope: cli/src/cmd/orchestrator.rs and focused tests. Wrap agent.reports and pane.list/pane metadata failures in orchestrator-specific messages instead of raw SocketCallError/RPC output. Add small extracted renderer/helper if useful. Suggested messages: cannot read orchestrator reports for pane -> run codeterm pane list; cannot reseed because pane is not an agent/has no session -> run codeterm pane list --agents and pass an agent pane. Do not touch unrelated plan renderer work. Run focused tests and git diff --check. Report in_review.
