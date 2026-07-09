---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_076d0404863b41deaf187142dbe6830d
handle: zero-flag-plan-context-resolve-plan-task-from-pane-binding
title: 'Zero-flag plan context: resolve plan/task from pane binding'
status: done
---

## Problem (observed in production, cross-machine)

Worker spawn briefs and plan reminders instruct agents to run commands with `$CODETERM_PLAN_ID` / `$CODETERM_TASK_ID` (e.g. `codeterm plan task list $CODETERM_PLAN_ID --json`, `codeterm agent report --plan $CODETERM_PLAN_ID --task $CODETERM_TASK_ID`). But those env vars are injected into pane env exactly once, at PTY spawn time (`src-tauri/src/service/pty_io/lifecycle.rs:54` via `plan_store.task_binding_for_pane`). The standard dispatch flow is spawn-first, assign-after (`plan task update --assign-pane`), so at spawn time there is no binding and the vars are never set — env cannot be injected into an already-running process. Result: the shell expands `$CODETERM_PLAN_ID` to empty and clap fails with `required arguments were not provided: <PLAN_ID>`. Screenshot evidence: worker "Codd" on peer machine, plan "Web UI v2 — PWA stabilization".

p1c (zero-flag report, commit 0eac3e01) already fixed this for `agent report` by resolving plan/task daemon-side from pane identity. Every other plan command still requires an explicit PLAN_ID.

## Fix (TDD — tests first)

1. **Extend zero-flag pane-identity resolution to plan CLI commands.** When the PLAN_ID positional/env is absent and the caller has `CODETERM_PANE_ID`, resolve plan (and task where relevant) daemon-side from the pane's task binding — same mechanism as p1c report resolution. Cover at minimum: `plan task list`, `plan task show/update`, `plan get`, `plan record`, `plan state`. When no binding exists, the error must be actionable: name the pane, say no active task binding was found, and show how to pass the id explicitly.
2. **Prompts stop referencing raw env vars as if guaranteed.** Update `src-tauri/src/service/plan/reminders.rs` and any spawn-brief/orchestration prompt text to use the zero-flag forms (`codeterm agent report "..."`, `codeterm plan task list`) instead of `$CODETERM_PLAN_ID` literals. Grep-sweep prompts/ and docs/ for `$CODETERM_PLAN_ID` usage presented as always-set.
3. **Late-binding env stays as a nice-to-have, not the contract.** Keep spawn-time injection for the assign-before-spawn path, but nothing in prompts may depend on it.

## TDD order

- RED: CLI/daemon test — `plan task list` with no PLAN_ID from a pane that HAS a binding resolves and lists tasks; from a pane WITHOUT a binding fails with the actionable error text.
- RED: reminder/prompt test asserting no `$CODETERM_PLAN_ID` literal in worker-facing reminder strings.
- GREEN: implement resolution + prompt updates.
- Regression: existing explicit-PLAN_ID invocations unchanged (positional wins over binding).
