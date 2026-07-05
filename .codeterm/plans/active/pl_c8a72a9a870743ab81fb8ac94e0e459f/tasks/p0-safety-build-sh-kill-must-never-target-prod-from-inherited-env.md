---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_62f626b6f3184b3b95769af8c1505344
handle: p0-safety-build-sh-kill-must-never-target-prod-from-inherited-env
title: P0 Safety — build.sh kill must never target prod from inherited env
status: done
tags:
- safety
- build
- dogfood
---

Additional accepted safety follow-up:
- build.sh web watcher cleanup now kills the pidfile process tree instead of only the parent pid.
- kill_web_watch also removes orphan `vite build --watch` processes whose cwd is exactly the current worktree, preventing stale watcher accumulation across dev restarts.
- Verification: bash -n build.sh; bash -n scripts/check-build-kill-safety.sh; git diff --check; process scan confirms no `vite build --watch` processes remain for /Users/rollacode/Developer/codeterm, only separate /Users/rollacode/Developer/codeterm-int watchers remain.
