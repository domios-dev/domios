---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_bba9cae69b4a4fdd99bb2bdf0f6b653a
handle: r3-fix-require-confirmation-for-plan-delete-cli
title: 'R3 fix: require confirmation for plan delete CLI'
status: done
---

HIGH finding from R3 CLI review. New top-level `codeterm plan delete <plan_id>` is destructive and has no confirmation flag. Evidence: `cli/src/args/plan.rs` defines `Delete { plan_id }`; `cli/src/cmd/plan.rs:566-567` calls `plan.delete` immediately; daemon `PlanStore::delete` removes the plan JSON and body dir. Implement a CLI guard requiring explicit confirmation, e.g. `codeterm plan delete <plan_id> --yes` or `--confirm`, and refuse with a clear non-zero error otherwise. Do not change archive/unarchive semantics. Add/adjust CLI parse/behavior tests proving delete without confirmation refuses before RPC and delete with confirmation sends the RPC.
