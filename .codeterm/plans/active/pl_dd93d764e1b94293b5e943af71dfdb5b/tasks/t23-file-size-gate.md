---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_2c487a7941d54f5eb180a2a37adb1454
handle: t23-file-size-gate
title: File-size gate cleanup after t7 review
status: done
---

Address t7 review file-size gate failures without changing behavior. Split oversized/regressed files named by review: packages/chat-ui/src/MessageBubble.tsx, src-tauri/src/service/plan/mod.rs, src-tauri/src/socket/server/plan.rs, src-tauri/src/service/plan/tests.rs. Prefer extracting cohesive helpers/modules; no allowlist growth. Preserve public APIs and tests. Coordinate with active t19 lifecycle lane before touching overlapping plan modules if needed.
