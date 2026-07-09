---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_47e5780428eb4e109c4b415f674e78eb
handle: p0-safety-build-sh-kill-must-never-target-prod-from-inherited-env-ms_47e5780428eb4e109c4b415f674e78eb
title: status_update
---

Accepted. build.sh now hard-pins canonical dev kill to 7686, refuses prod port 7685 in setup and kill_dev, and watcher cleanup targets the current worktree. Verified locally: bash -n build.sh; bash -n scripts/check-build-kill-safety.sh; scripts/check-build-kill-safety.sh; git diff --check; ./build.sh check.
