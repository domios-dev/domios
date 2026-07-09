---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_c946f69ef91d44e186f0f2d195f1f0d5
handle: mergeable-storage-research-ms_c946f69ef91d44e186f0f2d195f1f0d5
title: decision
---

SLOW STRUCTURAL/PROSE = one .md per section/task with YAML frontmatter (all scalar fields in frontmatter, prose body below) — extends §17 verbatim, human-readable + PR-reviewable. plan.json shrinks to scalar meta only (id,title,status,owner,spec_refs) — NO tasks/sections/strides arrays. Slow mutable fields (status,priority,section_ids) = last-writer-wins by updated_at, lamport tie-break; high-frequency state (status transitions, canvas moves) optionally modeled as events + folded.
