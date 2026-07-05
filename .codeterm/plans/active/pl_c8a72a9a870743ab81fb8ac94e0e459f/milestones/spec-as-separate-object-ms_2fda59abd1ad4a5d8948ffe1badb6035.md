---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_2fda59abd1ad4a5d8948ffe1badb6035
handle: spec-as-separate-object-ms_2fda59abd1ad4a5d8948ffe1badb6035
title: status_update
---

Unblocked by current trunk and accepted as done.

Evidence checked:
- Plan data model has spec_refs, spec_links, primary_spec_id in src-tauri/src/plan/types.rs.
- Backend create/link paths exist: create_plan_from_spec, link_spec_section, list_spec_links in src-tauri/src/service/plan/spec.rs.
- REST/socket surfaces exist for /api/plans/{id}/spec-links and plan create from_spec.
- Canvas consumes primary_spec_id/spec_links and renders Spec/Plan relationship.

This is no longer blocked on old supervisor uncommitted diffs.
