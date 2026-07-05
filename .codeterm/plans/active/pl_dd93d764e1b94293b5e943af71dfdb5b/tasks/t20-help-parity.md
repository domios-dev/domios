---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6ee55998fe2443b38600d7f9a5c107c5
handle: t20-help-parity
title: 'Help/docs parity: tier-1 homes for prompt knowledge'
status: done
---

Help/docs parity — give every prompt-inlined fact a tier-1 home BEFORE the prompt shrink (spec section prompt-diet; wave w16 part A). NO prompt removals in this task.

Steps:
1. Inventory codeterm_cli.md chunk by chunk: for each, record its tier-1 home (existing --help / docs topic) or create one.
2. Known gaps to fill: send --actions key vocabulary + delay semantics -> `codeterm send --help` long_help; pane backing-types table (pty vs chat_backend, no-shell rule) -> `codeterm pane --help` + docs; spawn shapes (Worker vs Orchestrator) + task-is-not-a-marker note -> `codeterm agent spawn --help` long_help; routing marker table -> new docs topic ROUTING (or CLI-doc section) registered in the embedded docs list.
3. Keep clap help terse per line but complete (long_help, not description bloat); verify `codeterm docs` lists any new topic.
4. Tests: cli manifest test still green; new docs topic readable via `codeterm docs ROUTING`; help snapshots for the extended commands.
Deliverable for t21: a mapping table (prompt chunk -> tier-1 home) in the report.
Commands: ./build.sh check; narrowed cargo test (cli); ./build.sh cli; manual --help/docs spot checks.
