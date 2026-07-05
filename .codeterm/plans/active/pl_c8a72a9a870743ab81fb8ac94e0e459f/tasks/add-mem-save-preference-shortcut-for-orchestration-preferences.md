---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_dc33b0e9cd51465f88b05f4027216a5d
handle: add-mem-save-preference-shortcut-for-orchestration-preferences
title: Add mem save-preference shortcut for orchestration preferences
status: done
---

Add a first-class convenience path for durable orchestration preferences.

Problem from plan.md:
Agents should save durable user/machine planning preferences into mem instead of relying on chat context, but the current command is generic and easy to misuse.

Scope:
- Add a small CLI alias or subcommand for saving preferences, e.g. `codeterm mem save-preference --title ... "body"` or an equally clean product-native form.
- The command must store kind=preference, sensible scope default, and optional workspace scope.
- Update ORCHESTRATION docs and prompt hints to use the new command.
- Keep `mem save` as the primitive; this task is about ergonomics, not a second storage path.

Acceptance:
- Fresh agents get a valid, easy command for saving local planning preferences.
- Existing mem tests still pass.
- CLI docs/manifest regenerated.

Tests:
- cargo test --manifest-path cli/Cargo.toml mem -- --nocapture
- cargo test --manifest-path cli/Cargo.toml cmd::manifest -- --nocapture
