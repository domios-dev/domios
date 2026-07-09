---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_c189839e9063458eb7aee8737edd0f85
handle: f1-plan-recall-index-mem-sidecar-ms_c189839e9063458eb7aee8737edd0f85
title: decision
---

ANDREY LOCKED CLI: UNIFY search into mem, keep plan related. (1) codeterm mem search searches global mem + in-project plan index (when in a project), results tagged [mem]/[plan], reuses mem filters (--kind task|section|stride|spec, --project, --limit). Implementation = extend the mem STORE with a PROJECT-LOCAL db under .codeterm/ (alongside the global one); plan units mirrored there embed-only, scope=plan_<project_root_hash>. Satisfies store-in-project + one-search-verb + reuse-mem-engine. (2) codeterm plan related --id <unit> = structural edge traversal (section_ids, subtasks parent_task_id, spec_links, lineage), dangling-safe — stays in plan ns (traverse, not search). (3) NO standalone plan search / plan similar (avoid 2nd search verb + CLI bloat). Worker cd1e3849 re-tasked.
