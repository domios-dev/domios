---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: tasks/cleanup-doctor-for-orchestration-lanes.md
handle: cleanup-doctor-for-orchestration-lanes
title: tasks/cleanup-doctor-for-orchestration-lanes.md
---
Add a local cleanup doctor command or script that reports safe cleanup candidates for the current orchestrator: finished worker panes, stale no-session panes, remote worker ghosts, lane worktrees/branches whose tasks are done, detached build-cache worktrees, dev/watch panes, dirty plan artifacts, and cache cleanup suggestions. Default dry-run only. Include exact commands and risk classification; do not archive plans automatically; never target prod 7685.
