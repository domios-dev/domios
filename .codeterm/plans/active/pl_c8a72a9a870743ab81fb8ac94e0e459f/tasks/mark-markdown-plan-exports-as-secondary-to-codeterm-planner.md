---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_36c8f08220b648a99d403058cb1bbe9a
handle: mark-markdown-plan-exports-as-secondary-to-codeterm-planner
title: Mark markdown plan exports as secondary to CodeTerm planner
status: done
---

Add source-of-truth markers for markdown plan exports.

Problem from plan.md:
Agents may create markdown implementation plans and accidentally treat them as authoritative. If markdown export exists, it must clearly say planner state is source of truth.

Scope:
- Audit any plan export/render paths and add a clear marker: generated/exported from CodeTerm planner; update planner, not this file.
- Document that markdown plans are secondary/export artifacts only.
- If export command exists, ensure it includes plan id/spec id and timestamp/source metadata.

Acceptance:
- Generated markdown cannot be mistaken for primary state.
- docs ORCHESTRATION states the rule.
- Existing render tests updated or added.

Tests:
- cargo test --manifest-path cli/Cargo.toml plan -- --nocapture
- cargo test --manifest-path cli/Cargo.toml cmd::manifest -- --nocapture
