---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5a6edbcc3ed344c492faa11cd63b073e
handle: make-plan-approve-run-or-enforce-quality-doctor
title: Make plan approve run or enforce quality doctor
status: done
---

Make approval UX hard to misuse.

Problem from plan.md/current slice:
`plan approve` exists, but the operator must remember to run `plan doctor --kind quality` first. The product should guide or enforce the approval gate more directly.

Scope:
- Decide the cleanest product behavior: CLI `plan approve` runs local doctor first, or daemon approval requires a quality-ready marker, or approve has an explicit `--skip-doctor` escape hatch.
- Avoid legacy support and avoid silent approval of underplanned work.
- Keep behavior deterministic for non-local/socket contexts.
- Update docs/prompts/tests.

Acceptance:
- A fresh orchestrator cannot accidentally approve a low-quality plan without a clear warning or explicit override.
- The error message includes exact recovery commands.
- Tests cover success, failure, and explicit override if any.

Tests:
- cargo test --manifest-path cli/Cargo.toml plan_doctor -- --nocapture
- cargo test --manifest-path cli/Cargo.toml plan_planning_gate_tests -- --nocapture
- cargo test --manifest-path cli/Cargo.toml cmd::manifest -- --nocapture
