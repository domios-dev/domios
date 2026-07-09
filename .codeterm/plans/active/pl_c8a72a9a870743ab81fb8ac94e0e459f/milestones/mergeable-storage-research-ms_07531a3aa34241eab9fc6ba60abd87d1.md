---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_07531a3aa34241eab9fc6ba60abd87d1
handle: mergeable-storage-research-ms_07531a3aa34241eab9fc6ba60abd87d1
title: decision
---

STRIDES = append-only event log. Per-task strides.jsonl with .gitattributes 'merge=union' (git built-in driver concatenates both branches' appended lines). Strides are immutable, ULID/time-prefixed ids. Deterministic fold = parse -> dedup by id -> sort by (lamport/ts,id) = the timeline. Two worktrees recording strides concurrently can NEVER conflict. One-file-per-stride is the fallback if union proves lossy. Strides target spec/plan/task -> shard log by target.
