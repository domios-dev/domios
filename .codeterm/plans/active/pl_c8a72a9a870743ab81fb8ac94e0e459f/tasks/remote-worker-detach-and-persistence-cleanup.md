---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: tasks/remote-worker-detach-and-persistence-cleanup.md
handle: remote-worker-detach-and-persistence-cleanup
title: tasks/remote-worker-detach-and-persistence-cleanup.md
---
Add/test a way to detach/forget remote workers from an orchestrator without closing remote panes blindly. It should remove persisted registry entries, emit remote-workers-changed, and be available via CLI/API/UI or cleanup doctor apply path. Preserve active recent workers unless explicitly selected.
