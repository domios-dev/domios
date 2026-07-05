---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4a72da152ae44354af28049942769ae2
handle: remove-canvas-message-composer
title: Remove Canvas plan message composer
status: done
---

Remove the inline Canvas plan message composer/input shown over the Canvas surface (placeholder like 'Message <pane>...', send button, plan/pane badge shell). This UX is rejected and should not remain as hidden legacy. Delete associated rendering, state, styles, and tests that exist only for this mechanism. Preserve normal task/plan detail overlays, activity indicators, and worker reports. If backend/API code exists only to support Canvas messages, remove or disconnect it unless used elsewhere.
