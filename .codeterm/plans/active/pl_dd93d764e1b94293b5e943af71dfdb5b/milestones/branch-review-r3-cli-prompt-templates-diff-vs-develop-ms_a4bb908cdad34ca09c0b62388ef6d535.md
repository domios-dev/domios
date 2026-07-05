---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_a4bb908cdad34ca09c0b62388ef6d535
handle: branch-review-r3-cli-prompt-templates-diff-vs-develop-ms_a4bb908cdad34ca09c0b62388ef6d535
title: finding
---

HIGH R3 CLI: new `codeterm plan delete <plan_id>` permanently removes plan JSON/body data via plan.delete -> PlanStore::delete without any CLI confirmation flag. Evidence: cli/src/args/plan.rs defines Delete { plan_id } only; cli/src/cmd/plan.rs:566-567 calls plan.delete immediately; src-tauri/src/service/plan/mod.rs:786-794 removes the plan file and body dir. Fix: add an explicit --yes/--confirm guard for plan delete (and tests proving delete refuses without it) before merge.
