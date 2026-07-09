---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_737967ceae43491dba812103a96e8f3e
handle: f1-plan-recall-index-mem-sidecar-ms_737967ceae43491dba812103a96e8f3e
title: decision
---

ANDREY greenlit plan search WITH vectors now (codex, TDD). DESIGN LOCKED: in-project index (.codeterm/plan-index.sqlite, FTS5+sqlite-vec+RRF, embed-only, scope=plan_<project_root_hash>). Unit=chunk, enriched text = parent(plan|spec) title + section ordinal N/total + own title + body. Update via daemon write-hook + existing file-watcher; per-unit upsert by stable id (no orphan vectors); delete on removal/archive. RELATED = traverse EXISTING plan edges (section_ids, parent_task_id subtasks, spec_links, lineage) — NOT a mem graph — DANGLING-SAFE (skip deleted targets, never panic). SIMILAR = vector kNN. CLI: codeterm plan search|related|similar; project default from CWD, --project to override. NO extraction/graph/dreaming over units (§21).
