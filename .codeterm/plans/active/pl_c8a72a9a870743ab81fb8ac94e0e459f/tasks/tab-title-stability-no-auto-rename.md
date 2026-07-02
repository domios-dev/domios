---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_b20455fdfd40410080e10511a3e0b7a9
handle: tab-title-stability-no-auto-rename
title: 'Tab title stability: remove automatic rename on unselect'
status: done
---

Remove unintended automatic tab renaming on unselect/focus changes. Valid title sources: initial title assigned at creation/spawn, explicit user rename, or explicit agent self-rename. No passive detector/focus/unselect path should overwrite a user-level title.
