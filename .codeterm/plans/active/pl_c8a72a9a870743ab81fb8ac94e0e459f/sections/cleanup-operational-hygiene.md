---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: sections/cleanup-operational-hygiene.md
handle: cleanup-operational-hygiene
title: sections/cleanup-operational-hygiene.md
---
Make CodeTerm orchestration cleanup boring and repeatable: stale panes/workers, remote worker ghosts, worktrees/branches, dev daemons/watchers, plan artifacts, and build/dev caches should be visible, classifiable, and cleaned only when safe. No automatic plan archival; no prod kill; destructive cleanup requires explicit owner approval or a dry-run/apply split.
