---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_01c9fac28d7b454eb91dcb96f26680b2
handle: rex-persistence-fix
title: Rex — persistence fix
status: done
---

Plans on disk were not re-indexed after restart. Added lazy project-dir scan on list, startup scan of open-workspace CWDs, retained watchers for pre-existing dirs, plus a regression test for a directly-dropped plan JSON.
