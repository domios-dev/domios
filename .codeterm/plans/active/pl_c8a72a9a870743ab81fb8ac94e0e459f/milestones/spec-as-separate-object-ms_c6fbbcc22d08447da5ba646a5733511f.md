---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_c6fbbcc22d08447da5ba646a5733511f
handle: spec-as-separate-object-ms_c6fbbcc22d08447da5ba646a5733511f
title: decision
---

ANDREY DECISION: Spec and Plan must be TWO DIFFERENT OBJECTS, not two views of one section set. Today: one Plan{sections} rendered as SPEC-node (prose view) + PLAN-node (structural view) — same sections, FE SectionNode variant. Target: a separate Spec entity with its OWN sections (prose), a Plan with its OWN task-bearing sections, connected by explicit typed edges (spec_section ↔ plan_section). Brainstorm §29 (data model/storage/migration, link model, coverage across two objects, optionality, CLI, canvas render generalizing §26 worlds).
