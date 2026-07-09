---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_f3ce9d2366db413fbf6bc4e5bd246df7
handle: mergeable-storage-research-ms_f3ce9d2366db413fbf6bc4e5bd246df7
title: decision
---

MERGE-STORAGE synth (opus 56957af8, 2/2 — converges w/ codex). The ONLY non-mergeable artifact today = monolithic <plan_id>.json (tasks[]+embedded strides). Bones exist (§17 .md = file-per-entity, §21 sqlite=derived) — finish decomposition. LAYOUT: plan.json=slim scalar meta; sections/<id>.md + tasks/<id>.md (frontmatter+body, extends §17); strides/<task>.jsonl (append-only ULID); links/+canvas/ one-file-per-entity; INDEX.md generated; sqlite+vectors gitignored. MERGE: git DEFAULT 3-way per-file + BUILT-IN merge=union for *.jsonl (.gitattributes) — NO custom driver, NO CRDT, NO Dolt. Two adds=two files=auto-merge; immutable stride events never conflict (fold=parse->dedup-by-id->sort ts,id). Concurrency: single-writer per-worktree; git IS cross-worktree merge. Mem: workspace facts=.md under .codeterm/mem/<area>/ (git-travel); global/identity stay home-sqlite. Migration: idempotent deterministic 'explode' on load (§17.3 migrated-flag). REJECTED CRDT/Dolt/pure-event-sourcing. SPIKE: verify git built-in union on EOF-append hunks before jsonl vs one-file-per-stride. BUILD ORDER: explode migration -> per-entity write path -> strides.jsonl+merge=union -> post-merge reindex -> mem workspace-fact files.
