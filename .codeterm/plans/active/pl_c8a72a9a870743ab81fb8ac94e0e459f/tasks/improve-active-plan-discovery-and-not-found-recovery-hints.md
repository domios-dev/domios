---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_3f4fdcae9c2d43f59fc7b3635d8f8302
handle: improve-active-plan-discovery-and-not-found-recovery-hints
title: Improve active-plan discovery and not-found recovery hints
status: done
---

Make active-plan discovery and plan-not-found errors actionable.

Problem from plan.md:
Agents often fail with a plan id/context mismatch and do not know whether they are on the wrong daemon, workspace, project path, or owner pane.

Scope:
- Improve CLI errors for missing plan/active plan resolution with exact diagnostic commands.
- Include current daemon identity, workspace, project path hints where available.
- Suggest `codeterm plan list --active --workspace`, `codeterm plan state`, and cross-daemon checks.
- Keep output concise; no nudge spam.

Acceptance:
- A raw agent can recover from a wrong/missing plan id without asking the user.
- Error renderer has tests.
- No broad daemon changes unless needed.

Tests:
- cargo test --manifest-path cli/Cargo.toml plan -- --nocapture
- cargo test --manifest-path cli/Cargo.toml cmd::manifest -- --nocapture
