---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_84849ae7e1484352bad6dc13a4097de0
handle: audit-planner-cli-error-explanations
title: Audit planner CLI error explanations
status: done
---

You are a CodeTerm Worker doing a read-only audit. Do not edit files. Do not spawn agents.

Task: audit CodeTerm planner/orchestrator CLI error messages for raw RPC leaks or vague guidance.

Context:
A user ran `codeterm plan record ... --status done` on an in_review task and got raw `RPC error -32603: task status transition InReview -> Done is not allowed`. That is a product bug in CLI explanation. We need find similar cases.

Scope:
- Inspect CLI renderers around `cli/src/cmd/plan_errors.rs`, plan/task/orchestrator commands, and relevant backend `PlanError::to_json` variants.
- Find backend structured errors that do not have human-friendly CLI renderers.
- Find CLI paths that bypass `render_plan_call_error` and would show raw RPC/transport text for common planner/orchestrator workflows.
- Include exact file/line references and recommended message shape/commands.
- Do NOT modify files. This is an adversarial audit report only.

Acceptance:
- Report lists every missing/weak renderer you can find, ordered by user impact.
- Confirm whether transition_denied renderer draft covers the known issue.
- Mention tests that should be added for each finding.

Run only cheap read-only commands: rg, sed, git diff, cargo test only if already needed and quick. No broad build.

Report by running:
codeterm agent report --plan pl_c8a72a9a870743ab81fb8ac94e0e459f --task task_REPLACE --status in_review "STATUS: in_review\nCHANGES: none (read-only audit)\nBUILD/TYPECHECK/TESTS: ...\nFINDINGS: ...\nFOLLOWUP: ..."
