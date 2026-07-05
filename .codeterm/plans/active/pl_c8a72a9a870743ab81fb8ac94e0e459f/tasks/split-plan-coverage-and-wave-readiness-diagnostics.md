---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_c36647cce434400d9a806f037fae490c
handle: split-plan-coverage-and-wave-readiness-diagnostics
title: Split plan coverage and wave readiness diagnostics
status: done
---

Extend plan doctor/state with wave workflow and better coverage diagnostics.

Problem from plan.md:
Plan quality should make discovery/research/approval/implementation waves visible, and coverage should distinguish spec coverage from task coverage instead of one vague percentage.

Scope:
- Improve `plan state` and/or `plan doctor --kind quality` output to show spec-link coverage, plan-section task coverage, and task-quality coverage separately.
- Add wave workflow guidance to docs/prompts and doctor findings where absent.
- Ensure findings are concise and actionable, not noisy.

Acceptance:
- A plan can be evaluated before tasks exist without looking like failed execution coverage.
- After tasks exist, unlinked/under-specified tasks are still hard errors.
- Output helps agents decide whether to do discovery, approval, implementation, or verification next.

Tests:
- cargo test --manifest-path cli/Cargo.toml plan_state -- --nocapture
- cargo test --manifest-path cli/Cargo.toml plan_doctor -- --nocapture
