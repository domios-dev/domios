---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_715668279089416793252eb1c175bca2
handle: add-rest-parity-for-planning-reset-and-element-mutations
title: Add REST parity for planning reset and element mutations
status: done
---

Bring REST parity to the new planning mutation surface.

Problem from plan.md/current slice:
Socket/CLI now covers task reset and note/element mutation, but REST should not lag behind for product/web integrations.

Scope:
- Add REST support for resetting generated tasks (`DELETE /api/plans/{id}/tasks` or equivalent explicit route) with an intentional confirmation field/query.
- Verify note/element update/rm REST routes are present and documented.
- Ensure REST errors use the same PlanError JSON for planning approval gates.
- Update docs/API.md if the route is public.

Acceptance:
- Socket and REST expose equivalent mutation capabilities for the planning model.
- Task reset returns removed count and resets plan to planning mode.
- Tests cover at least service-level semantics; route tests if existing harness supports them.

Tests:
- cargo test --manifest-path src-tauri/Cargo.toml service::plan::tests::task_creation_requires_plan_approval_unless_forced --lib -- --nocapture
- cargo check --manifest-path src-tauri/Cargo.toml
- CODETERM_API_PORT=7686 ./build.sh check
