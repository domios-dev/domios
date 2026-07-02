---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_cf4dfe4673b04a20b045ba3310f42997
handle: mergeable-storage-research-ms_cf4dfe4673b04a20b045ba3310f42997
title: decision
---

SINGLE-WRITER IS PER-WORKTREE; GIT IS THE CROSS-WORKTREE MERGE. Keep existing per-daemon serialized writer + atomic temp+rename + watcher rehydrate within a worktree (now cheaper). Across worktrees conflict resolution defers to git merge-time, trivial via file-per-entity + immutable events. No cross-process lock; git is the async CRDT.
