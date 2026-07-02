---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d691c8f97d794dd89df772cf6a646fec
handle: mergeable-storage-research-ms_d691c8f97d794dd89df772cf6a646fec
title: decision
---

SQLITE INDEX + VECTORS ALWAYS DERIVED, gitignored, never in git (confirms §21). Rebuilt by existing reindex_missing on file-watcher rehydrate / post-merge. CHEAP + incremental: content-hash column skips unchanged bodies, only changed files re-embed; plan-scale FTS5/search-unit rebuild is sub-second. Daemon already watches .codeterm/ -> a git merge that changes files auto-triggers rehydrate+reindex; explicit post-merge hook is optional.
